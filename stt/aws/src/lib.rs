use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};

use golem_stt::error::Error;

use golem_stt::golem::stt::transcription::{
    FailedTranscription as WitFailedTranscription, Guest as TranscriptionGuest,
    MultiTranscriptionResult as WitMultiTranscriptionResult,
    TranscribeOptions as WitTranscribeOptions, TranscriptionRequest as WitTranscriptionRequest,
    TranscriptionResult as WitTranscriptionResult,
};

use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError as WitSttError, TimingInfo as WitTimingInfo,
    TimingMarkType as WitTimingMarkType, TranscriptAlternative as WitTranscriptAlternative,
    TranscriptionMetadata as WitTranscriptionMetadata, WordSegment as WitWordSegment,
};
use golem_stt::transcription::SttProviderClient;
use golem_stt::LOGGING_STATE;
use log::trace;
use transcription::api::{TranscribeApi, TranscriptionResponse};
use transcription::request::{AudioConfig, AudioFormat, TranscriptionConfig, TranscriptionRequest};

use futures_concurrency::future::Join;
use itertools::Itertools;
use wstd::runtime::block_on;

mod transcription;

#[allow(unused)]
struct Component;

impl LanguageGuest for Component {
    fn list_languages() -> Result<Vec<LanguageInfo>, WitSttError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        let supported_languages = transcription::api::get_supported_languages();
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

        let region = std::env::var("AWS_REGION").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_REGION: {}", err))
        })?;

        let access_key = std::env::var("AWS_ACCESS_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_ACCESS_KEY: {}", err))
        })?;

        let secret_key = std::env::var("AWS_SECRET_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_SECRET_KEY: {}", err))
        })?;

        let bucket_name = std::env::var("AWS_BUCKET_NAME").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_BUCKET_NAME: {}", err))
        })?;

        block_on(async {
            let api_client = TranscribeApi::live(bucket_name, access_key, secret_key, region);

            let api_response = api_client.transcribe_audio(req.try_into()?).await?;

            Ok(api_response.into())
        })
    }

    fn transcribe_many(
        wit_requests: Vec<WitTranscriptionRequest>,
    ) -> Result<WitMultiTranscriptionResult, WitSttError> {
        LOGGING_STATE.with_borrow_mut(|state| state.init());

        let region = std::env::var("AWS_REGION").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_REGION: {}", err))
        })?;

        let access_key = std::env::var("AWS_ACCESS_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_ACCESS_KEY: {}", err))
        })?;

        let secret_key = std::env::var("AWS_SECRET_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_SECRET_KEY: {}", err))
        })?;

        let bucket_name = std::env::var("AWS_BUCKET_NAME").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_BUCKET_NAME: {}", err))
        })?;

        block_on(async {
            let api_client = TranscribeApi::live(bucket_name, access_key, secret_key, region);

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

            // Might need to enable this if https://github.com/golemcloud/golem/issues/1865 does not get fixed
            // for request in requests {
            //     let res = api_client.transcribe_audio(request).await; // returns a Result<TranscriptionResponse, TranscriptionError>
            //     match res {
            //         Ok(resp) => successes.push(resp.into()),
            //         Err(err) => {
            //             trace!("transcription request failed, error {}", err);
            //             failures.push(WitFailedTranscription {
            //                 request_id: err.request_id().to_string(),
            //                 error: WitSttError::from(err),
            //             });
            //         }
            //     }
            // }

            for chunk in requests.into_iter().chunks(32).into_iter() {
                let req_vec: Vec<_> = chunk.collect();

                let futures = req_vec
                    .into_iter()
                    .map(|request| api_client.transcribe_audio(request))
                    .collect::<Vec<_>>();

                trace!("waiting for transcription jobs to complete");
                let results = futures.join().await;
                trace!("transcription job completed");

                for res in results {
                    match res {
                        Ok(resp) => successes.push(resp.into()),
                        Err(err) => {
                            trace!("transcription request failed, error {}", err);
                            failures.push(WitFailedTranscription {
                                request_id: err.request_id().to_string(),
                                error: WitSttError::from(err),
                            })
                        }
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
        if let Some(language_code) = &options.language {
            if !transcription::api::is_supported_language(language_code) {
                return Err(WitSttError::UnsupportedLanguage(language_code.clone()));
            }
        }

        let vocabulary: Vec<_> = options
            .vocabulary
            .map(|vocab| {
                vocab
                    .phrases
                    .into_iter()
                    .map(|phrase| phrase.value)
                    .collect()
            })
            .unwrap_or_default();

        Ok(TranscriptionConfig {
            language: options.language,
            model: options.model,
            enable_speaker_diarization: options.enable_speaker_diarization.unwrap_or(false),
            vocabulary,
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
        let aws_results = &response.aws_transcription.results;

        // AWS Transcription result does not contain duration information, so we calculate it from the last item's end time
        let duration_seconds = aws_results
            .items
            .last()
            .and_then(|item| item.end_time.as_ref())
            .and_then(|time_str| time_str.parse::<f32>().ok())
            .unwrap_or(0.0);

        let metadata = WitTranscriptionMetadata {
            duration_seconds,
            audio_size_bytes: response.audio_size_bytes as u32,
            request_id: response.aws_transcription.job_name,
            model: response.model,
            language: response.language,
        };

        let alternatives: Vec<WitTranscriptAlternative> = aws_results
            .transcripts
            .iter()
            .map(|transcript| {
                // Create word segments from items (pronunciation only)
                let words: Vec<WitWordSegment> = aws_results
                    .items
                    .iter()
                    .filter(|item| item.item_type == "pronunciation")
                    .filter_map(|item| {
                        // Get the best alternative (first one)
                        let alternative = item.alternatives.first()?;

                        // Parse confidence from string
                        let confidence = alternative.confidence.parse::<f32>().ok();

                        // Create timing info if available
                        let timing_info = match (&item.start_time, &item.end_time) {
                            (Some(start_str), Some(end_str)) => {
                                match (start_str.parse::<f32>(), end_str.parse::<f32>()) {
                                    (Ok(start), Ok(end)) => Some(WitTimingInfo {
                                        start_time_seconds: start,
                                        end_time_seconds: end,
                                        mark_type: WitTimingMarkType::Word,
                                    }),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };

                        Some(WitWordSegment {
                            text: alternative.content.clone(),
                            timing_info,
                            confidence,
                            speaker_id: item.speaker_label.clone(),
                        })
                    })
                    .collect();

                // Calculate average confidence for the alternative
                let confidence = if words.is_empty() {
                    0.0
                } else {
                    let sum: f32 = words.iter().filter_map(|word| word.confidence).sum();
                    let count = words
                        .iter()
                        .filter(|word| word.confidence.is_some())
                        .count();
                    if count > 0 {
                        sum / count as f32
                    } else {
                        0.0
                    }
                };

                WitTranscriptAlternative {
                    text: transcript.transcript.clone(),
                    confidence,
                    words,
                }
            })
            .collect();

        WitTranscriptionResult {
            metadata,
            alternatives,
        }
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
