use futures_concurrency::future::Join;
use golem_stt::transcription::SttProviderClient;
use itertools::Itertools;
use wstd::runtime::block_on;

mod transcription;

use golem_stt::error::Error as SttError;
use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError as WitSttError, TimingInfo as WitTimingInfo,
    TimingMarkType as WitTimingMarkType, TranscriptAlternative as WitTranscriptAlternative,
    TranscriptionMetadata as WitTranscriptionMetadata, WordSegment as WitWordSegment,
};
use golem_stt::http::WstdHttpClient;
use golem_stt::LOGGING_STATE;

use golem_stt::golem::stt::transcription::{
    FailedTranscription as WitFailedTranscription, Guest as TranscriptionGuest,
    MultiTranscriptionResult as WitMultiTranscriptionResult,
    TranscribeOptions as WitTranscribeOptions, TranscriptionRequest as WitTranscriptionRequest,
    TranscriptionResult as WitTranscriptionResult,
};

use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};
use transcription::{
    AudioConfig, AudioFormat, DiarizationConfig, FastTranscriptionApi, ProfanityFilterMode,
    TranscriptionConfig, TranscriptionRequest, TranscriptionResponse,
};
use wstd::time::Duration;

#[allow(unused)]
struct Component;

impl LanguageGuest for Component {
    fn list_languages() -> Result<Vec<LanguageInfo>, WitSttError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        let supported_languages = transcription::get_supported_languages();
        Ok(supported_languages
            .iter()
            .map(|lang| LanguageInfo {
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

        let region = std::env::var("AZURE_REGION").map_err(|err| {
            SttError::EnvVariablesNotSet(format!("Failed to load AZURE_REGION: {}", err))
        })?;

        let subscription_key = std::env::var("AZURE_SUBSCRIPTION_KEY").map_err(|err| {
            SttError::EnvVariablesNotSet(format!("Failed to load AZURE_SUBSCRIPTION_KEY: {}", err))
        })?;

        block_on(async {
            let api_client = FastTranscriptionApi::new(
                subscription_key,
                region,
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

        let region = std::env::var("AZURE_REGION").map_err(|err| {
            SttError::EnvVariablesNotSet(format!("Failed to load AZURE_REGION: {}", err))
        })?;

        let subscription_key = std::env::var("AZURE_SUBSCRIPTION_KEY").map_err(|err| {
            SttError::EnvVariablesNotSet(format!("Failed to load AZURE_SUBSCRIPTION_KEY: {}", err))
        })?;

        block_on(async {
            let api_client = FastTranscriptionApi::new(
                subscription_key,
                region,
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

            for chunk in requests.into_iter().chunks(16).into_iter() {
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

impl TryFrom<WitAudioFormat> for AudioFormat {
    type Error = WitSttError;

    fn try_from(wit_format: WitAudioFormat) -> Result<Self, Self::Error> {
        match wit_format {
            WitAudioFormat::Wav => Ok(AudioFormat::wav),
            WitAudioFormat::Mp3 => Ok(AudioFormat::mp3),
            WitAudioFormat::Flac => Ok(AudioFormat::flac),
            WitAudioFormat::Ogg => Ok(AudioFormat::ogg),
            WitAudioFormat::Aac => Ok(AudioFormat::aac),
            format => Err(WitSttError::UnsupportedFormat(format!(
                "{format:?}is not supported"
            ))),
        }
    }
}

impl TryFrom<WitTranscribeOptions> for TranscriptionConfig {
    type Error = WitSttError;

    fn try_from(options: WitTranscribeOptions) -> Result<Self, Self::Error> {
        if let Some(language_code) = &options.language {
            if transcription::is_supported_language(language_code) {
                return Err(WitSttError::UnsupportedLanguage(language_code.to_owned()));
            }
        }

        let diarization_config = if options.enable_speaker_diarization.unwrap_or(false) {
            Some(DiarizationConfig {
                enabled: true,
                max_speakers: 2, // TODO this needs to be configurable
            })
        } else {
            None
        };

        let profanity_filter_mode = if options.profanity_filter.unwrap_or(false) {
            Some(ProfanityFilterMode::Masked)
        } else {
            None
        };

        Ok(TranscriptionConfig {
            locales: options.language.map_or_else(Vec::new, |lang| vec![lang]),
            diarization: diarization_config,
            profanity_filter_mode,
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
                format: request.config.format.try_into()?,
                channels: request.config.channels,
            },
            transcription_config,
        })
    }
}

impl From<TranscriptionResponse> for WitTranscriptionResult {
    fn from(response: TranscriptionResponse) -> Self {
        let azure_transcription = &response.azure_transcription;

        let duration_seconds = azure_transcription.duration_milliseconds as f32 / 1000.0;

        let metadata = WitTranscriptionMetadata {
            duration_seconds,
            audio_size_bytes: response.audio_size_bytes as u32,
            request_id: response.request_id,
            model: None, // Azure Fast Transcription API doesn't expose model information
            language: response.locales.as_slice().join(", "),
        };

        let alternatives: Vec<WitTranscriptAlternative> = azure_transcription
            .phrases
            .iter()
            .map(|phrase| {
                let words: Vec<WitWordSegment> = phrase
                    .words
                    .iter()
                    .map(|word| WitWordSegment {
                        text: word.text.clone(),
                        timing_info: Some(WitTimingInfo {
                            start_time_seconds: word.offset_milliseconds as f32 / 1000.0,
                            end_time_seconds: (word.offset_milliseconds
                                + word.duration_milliseconds)
                                as f32
                                / 1000.0,
                            mark_type: WitTimingMarkType::Word,
                        }),
                        confidence: Some(phrase.confidence as f32),
                        speaker_id: phrase.speaker.map(|id| id.to_string()),
                    })
                    .collect();

                WitTranscriptAlternative {
                    text: phrase.text.clone(),
                    confidence: phrase.confidence as f32,
                    words,
                }
            })
            .collect();

        // If we have no phrase-level alternatives, create one from combined_phrases
        let alternatives_with_fallback = if alternatives.is_empty() {
            azure_transcription
                .combined_phrases
                .iter()
                .map(|combined_phrase| WitTranscriptAlternative {
                    text: combined_phrase.text.clone(),
                    confidence: 1.0, // Combined phrases don't have confidence scores
                    words: vec![],   // Combined phrases don't have word-level details
                })
                .collect()
        } else {
            alternatives
        };

        WitTranscriptionResult {
            metadata,
            alternatives: alternatives_with_fallback,
        }
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
