use futures_concurrency::future::Join;
use golem_stt::http::WstdHttpClient;
use golem_stt::transcription::SttProviderClient;
use golem_stt::LOGGING_STATE;
use itertools::Itertools;

use golem_stt::error::Error;
use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError as WitSttError, TimingInfo as WitTimingInfo,
    TimingMarkType as WitTimingMarkType, TranscriptAlternative as WitTranscriptAlternative,
    TranscriptionMetadata as WitTranscriptionMetadata, WordSegment as WitWordSegment,
};

use golem_stt::golem::stt::transcription::{
    FailedTranscription as WitFailedTranscription, Guest as TranscriptionGuest,
    MultiTranscriptionResult as WitMultiTranscriptionResult,
    TranscribeOptions as WitTranscribeOptions, TranscriptionRequest as WitTranscriptionRequest,
    TranscriptionResult as WitTranscriptionResult,
};

use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};
use transcription::{
    AudioConfig, AudioFormat, TranscriptionConfig, TranscriptionRequest, TranscriptionResponse,
    TranscriptionsApi, WhisperTranscription,
};
use wstd::runtime::block_on;

mod transcription;

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

        let api_key = std::env::var("OPENAI_API_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load OPENAI_API_KEY: {}", err))
        })?;

        block_on(async {
            let api_client = TranscriptionsApi::new(api_key, WstdHttpClient::default());

            let api_response = api_client.transcribe_audio(req.try_into()?).await?;

            Ok(api_response.into())
        })
    }

    fn transcribe_many(
        wit_requests: Vec<WitTranscriptionRequest>,
    ) -> Result<WitMultiTranscriptionResult, WitSttError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        let api_key = std::env::var("OPENAI_API_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load OPENAI_API_KEY: {}", err))
        })?;

        block_on(async {
            let api_client = TranscriptionsApi::new(api_key, WstdHttpClient::default());

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
            format => Err(WitSttError::UnsupportedFormat(format!(
                "{format:?}is not supported"
            ))),
        }
    }
}

impl TryFrom<WitTranscribeOptions> for TranscriptionConfig {
    type Error = WitSttError;

    fn try_from(options: WitTranscribeOptions) -> Result<Self, Self::Error> {
        let enable_timestamps = options.enable_timestamps.unwrap_or(false);

        let prompt = options.speech_context.map(|c| c.join(", "));

        if let Some(language_code) = &options.language {
            if transcription::is_supported_language(language_code) {
                return Err(WitSttError::UnsupportedLanguage(language_code.to_owned()));
            }
        }

        Ok(TranscriptionConfig {
            enable_timestamps,
            language: options.language,
            prompt,
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
            },
            transcription_config,
        })
    }
}

impl From<TranscriptionResponse> for WitTranscriptionResult {
    fn from(response: TranscriptionResponse) -> Self {
        match response.whisper_transcription {
            WhisperTranscription::Words {
                task: _,
                language,
                duration: _,
                text,
                words,
                usage,
            } => {
                let metadata = WitTranscriptionMetadata {
                    duration_seconds: usage.seconds as f32,
                    audio_size_bytes: response.audio_size_bytes as u32,
                    request_id: "".to_string(),
                    model: Some("whisper-1".to_string()),
                    language,
                };

                let wit_word_segments: Vec<WitWordSegment> = words
                    .into_iter()
                    .map(|word| WitWordSegment {
                        text: word.word,
                        timing_info: Some(WitTimingInfo {
                            start_time_seconds: word.start as f32,
                            end_time_seconds: word.end as f32,
                            mark_type: WitTimingMarkType::Word,
                        }),
                        confidence: None,
                        speaker_id: None,
                    })
                    .collect();

                let alternative = WitTranscriptAlternative {
                    text,
                    confidence: 0.0,
                    words: wit_word_segments,
                };

                WitTranscriptionResult {
                    metadata,
                    alternatives: vec![alternative],
                }
            }
            WhisperTranscription::Segments {
                task: _,
                language,
                duration: _,
                text,
                segments: _,
                usage,
            } => {
                let metadata = WitTranscriptionMetadata {
                    duration_seconds: usage.seconds as f32,
                    audio_size_bytes: response.audio_size_bytes as u32,
                    request_id: "".to_string(),
                    model: Some("whisper-1".to_string()),
                    language,
                };

                let alternative = WitTranscriptAlternative {
                    text,
                    confidence: 0.0,
                    words: vec![],
                };

                WitTranscriptionResult {
                    metadata,
                    alternatives: vec![alternative],
                }
            }
        }
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
