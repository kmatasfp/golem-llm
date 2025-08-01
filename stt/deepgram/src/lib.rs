use golem_stt::error::Error;
use golem_stt::http::WstdHttpClient;
use golem_stt::transcription::SttProviderClient;
use golem_stt::LOGGING_STATE;
use transcription::{
    AudioConfig, AudioFormat, Keyword, PreRecordedAudioApi, TranscriptionConfig,
    TranscriptionRequest, TranscriptionResponse,
};

use golem_stt::golem::stt::languages::{
    Guest as WitLanguageGuest, LanguageInfo as WitLanguageInfo,
};

use golem_stt::golem::stt::transcription::{
    FailedTranscription as WitFailedTranscription, Guest as TranscriptionGuest,
    MultiTranscriptionResult as WitMultiTranscriptionResult, Phrase as WitPhrase,
    TranscribeOptions as WitTranscribeOptions, TranscriptionRequest as WitTranscriptionRequest,
    Vocabulary as WitVocabulary,
};

use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError as WitSttError, TimingInfo as WitTimingInfo,
    TranscriptionChannel as WitTranscriptionChannel,
    TranscriptionMetadata as WitTranscriptionMetadata,
    TranscriptionResult as WitTranscriptionResult, TranscriptionSegment as WitTranscriptionSegment,
    WordSegment as WitWordSegment,
};

use futures_concurrency::future::Join;
use itertools::Itertools;
use wstd::runtime::block_on;
use wstd::time::Duration;

mod transcription;

#[allow(unused)]
struct Component;

impl WitLanguageGuest for Component {
    fn list_languages() -> Result<Vec<WitLanguageInfo>, WitSttError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        let supported_languages = transcription::get_supported_languages();
        Ok(supported_languages
            .iter()
            .map(|lang| WitLanguageInfo {
                code: lang.code.to_string(),
                name: lang.name.to_string(),
                native_name: lang.native_name.to_string(),
            })
            .collect())
    }
}

impl TranscriptionGuest for Component {
    fn transcribe(req: WitTranscriptionRequest) -> Result<WitTranscriptionResult, WitSttError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        let api_key = std::env::var("DEEPGRAM_API_TOKEN").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load DEEPGRAM_API_TOKEN: {}", err))
        })?;

        block_on(async {
            let api_client = PreRecordedAudioApi::new(
                api_key,
                WstdHttpClient::new_with_timeout(Duration::from_secs(60), Duration::from_secs(600)),
            );

            let api_response = api_client.transcribe_audio(req.try_into()?).await?;

            Ok(api_response.into())
        })
    }

    fn transcribe_many(
        wit_requests: Vec<WitTranscriptionRequest>,
    ) -> Result<WitMultiTranscriptionResult, WitSttError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        let api_key = std::env::var("DEEPGRAM_API_TOKEN").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load DEEPGRAM_API_TOKEN: {}", err))
        })?;

        block_on(async {
            let api_client = PreRecordedAudioApi::new(
                api_key,
                WstdHttpClient::new_with_timeout(Duration::from_secs(60), Duration::from_secs(600)),
            );

            let mut successes: Vec<WitTranscriptionResult> = Vec::new();
            let mut failures: Vec<WitFailedTranscription> = Vec::new();

            let requests: Vec<_> = wit_requests
                .into_iter()
                .map(|wr| (wr.request_id.clone(), TranscriptionRequest::try_from(wr)))
                .filter_map(|(id, res)| match res {
                    Ok(req) => Some(req),
                    Err(err) => {
                        failures.push(WitFailedTranscription {
                            request_id: id,
                            error: err,
                        });
                        None
                    }
                })
                .collect();

            for chunk in requests.into_iter().chunks(32).into_iter() {
                let req_vec: Vec<_> = chunk.collect();

                let futures = req_vec
                    .into_iter()
                    .map(|request| api_client.transcribe_audio(request))
                    .collect::<Vec<_>>();

                let results = futures.join().await;

                for res in results {
                    match res {
                        Ok(resp) => successes.push(resp.into()),
                        Err(err) => failures.push(WitFailedTranscription {
                            request_id: err.request_id().to_string(),
                            error: WitSttError::from(err),
                        }),
                    }
                }
            }

            Ok(WitMultiTranscriptionResult {
                successes,
                failures,
            })
        })
    }
}

impl From<WitAudioFormat> for AudioFormat {
    fn from(wit_format: WitAudioFormat) -> Self {
        match wit_format {
            WitAudioFormat::Wav => AudioFormat::wav,
            WitAudioFormat::Mp3 => AudioFormat::mp3,
            WitAudioFormat::Flac => AudioFormat::flac,
            WitAudioFormat::Ogg => AudioFormat::ogg,
            WitAudioFormat::Aac => AudioFormat::aac,
            WitAudioFormat::Pcm => AudioFormat::pcm,
        }
    }
}

impl TryFrom<WitTranscribeOptions> for TranscriptionConfig {
    type Error = WitSttError;

    fn try_from(options: WitTranscribeOptions) -> Result<Self, Self::Error> {
        fn map_vocabulary_phrases<T, F>(vocab: Option<WitVocabulary>, mapper: F) -> Vec<T>
        where
            F: Fn(WitPhrase) -> T,
        {
            vocab.map_or_else(Vec::new, |vocab| {
                vocab.phrases.into_iter().map(mapper).collect()
            })
        }

        fn to_keyterms(vocab: Option<WitVocabulary>) -> Vec<String> {
            map_vocabulary_phrases(vocab, |phrase| phrase.value)
        }

        fn to_keywords(vocab: Option<WitVocabulary>) -> Vec<Keyword> {
            map_vocabulary_phrases(vocab, |phrase| Keyword {
                value: phrase.value,
                boost: phrase.boost,
            })
        }

        if let Some(language_code) = &options.language {
            if crate::transcription::is_supported_language(language_code) {
                return Err(WitSttError::UnsupportedLanguage(language_code.to_owned()));
            }
        }

        let mut keyterms = Vec::new();
        let mut keywords = Vec::new();

        match &options.model {
            Some(model) => match model.as_str() {
                "nova-3" => {
                    if let Some(vocab) = options.vocabulary {
                        keyterms = to_keyterms(Some(vocab));
                    }
                }
                "nova-2" | "nova-1" | "enhanced" | "base" => {
                    if let Some(vocab) = options.vocabulary {
                        keywords = to_keywords(Some(vocab));
                    }
                }
                _ => (),
            },
            None => {
                if let Some(vocab) = options.vocabulary {
                    keywords = to_keywords(Some(vocab));
                }
            }
        }

        let enable_multi_channel = options.enable_multi_channel.unwrap_or(false);
        let enable_speaker_diarization = options
            .diarization
            .map(|diarization| diarization.enabled)
            .unwrap_or(false);

        Ok(TranscriptionConfig {
            language: options.language,
            model: options.model,
            enable_profanity_filter: options.profanity_filter.unwrap_or(false),
            enable_speaker_diarization,
            enable_multi_channel,
            keywords,
            keyterms,
        })
    }
}

impl TryFrom<WitTranscriptionRequest> for TranscriptionRequest {
    type Error = WitSttError;

    fn try_from(request: WitTranscriptionRequest) -> Result<Self, Self::Error> {
        let audio = request.audio;

        let transcription_config: Option<TranscriptionConfig> =
            if let Some(options) = request.options {
                Some(options.try_into()?)
            } else {
                None
            };

        Ok(TranscriptionRequest {
            request_id: request.request_id,
            audio,
            audio_config: AudioConfig {
                format: request.config.format.into(),
                channels: request.config.channels,
            },
            transcription_config,
        })
    }
}

impl From<TranscriptionResponse> for WitTranscriptionResult {
    fn from(response: TranscriptionResponse) -> Self {
        let metadata = WitTranscriptionMetadata {
            duration_seconds: response.deepgram_transcription.metadata.duration,
            audio_size_bytes: response.audio_size_bytes as u32,
            request_id: response.request_id,
            model: serde_json::to_string(&response.deepgram_transcription.metadata.model_info).ok(),
            language: response.language,
        };

        let wit_channels: Vec<_> = response
            .deepgram_transcription
            .results
            .channels
            .into_iter()
            .enumerate()
            .map(|(channel_idx, channel)| {
                // Get the channel transcript from the first alternative
                let channel_transcript = channel
                    .alternatives
                    .first()
                    .map(|alt| alt.transcript.clone())
                    .unwrap_or_default();

                // Filter utterances for this channel and convert to segments
                let wit_segments: Vec<_> = response
                    .deepgram_transcription
                    .results
                    .utterances
                    .iter()
                    .filter(|utterance| utterance.channel as usize == channel_idx)
                    .map(|utterance| {
                        let wit_words: Vec<_> = utterance
                            .words
                            .iter()
                            .map(|word| WitWordSegment {
                                text: word.word.clone(),
                                timing_info: Some(WitTimingInfo {
                                    start_time_seconds: word.start,
                                    end_time_seconds: word.end,
                                }),
                                confidence: Some(word.confidence),
                                speaker_id: word.speaker.map(|id| id.to_string()),
                            })
                            .collect();

                        WitTranscriptionSegment {
                            transcript: utterance.transcript.clone(),
                            timing_info: Some(WitTimingInfo {
                                start_time_seconds: utterance.start,
                                end_time_seconds: utterance.end,
                            }),
                            speaker_id: utterance.speaker.map(|id| id.to_string()),
                            words: wit_words,
                        }
                    })
                    .collect();

                WitTranscriptionChannel {
                    id: channel_idx.to_string(),
                    transcript: channel_transcript,
                    segments: wit_segments,
                }
            })
            .collect();

        WitTranscriptionResult {
            transcript_metadata: metadata,
            channels: wit_channels,
        }
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
