use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError, TimingInfo as WitTimingInfo,
    TimingMarkType as WitTimingMarkType, TranscriptAlternative as WitTranscriptAlternative,
    TranscriptionMetadata as WitTranscriptionMetadata, WordSegment as WitWordSegment,
};

use golem_stt::golem::stt::transcription::{
    TranscribeOptions as WitTranscribeOptions, TranscriptionResult as WitTranscriptionResult,
};

use crate::client::{
    AudioFormat, Error, TranscriptionConfig, TranscriptionResponse, WhisperTranscription,
};

use serde_json::to_string;

impl TryFrom<WitAudioFormat> for AudioFormat {
    type Error = SttError;

    fn try_from(wit_format: WitAudioFormat) -> Result<Self, Self::Error> {
        match wit_format {
            WitAudioFormat::Wav => Ok(AudioFormat::wav),
            WitAudioFormat::Mp3 => Ok(AudioFormat::mp3),
            WitAudioFormat::Flac => Ok(AudioFormat::flac),
            WitAudioFormat::Ogg => Ok(AudioFormat::ogg),
            format => Err(SttError::UnsupportedFormat(format!(
                "{format:?}is not supported"
            ))),
        }
    }
}

impl TryFrom<WitTranscribeOptions> for TranscriptionConfig {
    type Error = SttError;

    fn try_from(options: WitTranscribeOptions) -> Result<Self, Self::Error> {
        let enable_timestamps = options.enable_timestamps.unwrap_or(false);

        let prompt = options.speech_context.map(|c| c.join(", "));

        if let Some(language_code) = &options.language {
            if crate::client::WHISPER_SUPPORTED_LANGUAGES
                .iter()
                .find(|lang| lang.code == language_code)
                .is_none()
            {
                return Err(SttError::UnsupportedLanguage(language_code.to_owned()));
            }
        }

        Ok(TranscriptionConfig {
            enable_timestamps,
            language: options.language,
            prompt,
        })
    }
}

impl From<Error> for SttError {
    fn from(error: Error) -> Self {
        match error {
            Error::Reqwest(error) => SttError::NetworkError(format!("Failed to call API: {error}")),
            Error::SerdeJson(error) => {
                SttError::InternalError(format!("API returned unexpected JSON: {error}"))
            }
            Error::APIBadRequest { body } => SttError::InvalidAudio(to_string(&body).unwrap()),
            Error::APIUnauthorized { body } => SttError::AccessDenied(to_string(&body).unwrap()),
            Error::APIForbidden { body } => SttError::Unauthorized(to_string(&body).unwrap()),
            Error::APINotFound { body } => {
                SttError::UnsupportedOperation(to_string(&body).unwrap())
            }
            Error::APIConflict { body } => SttError::ServiceUnavailable(to_string(&body).unwrap()),
            Error::APIUnprocessableEntity { body } => {
                SttError::ServiceUnavailable(to_string(&body).unwrap())
            }
            Error::APIRateLimit { body } => SttError::RateLimited(to_string(&body).unwrap()),
            Error::APIInternalServerError { body } => {
                SttError::ServiceUnavailable(to_string(&body).unwrap())
            }
            Error::APIUnknown { body } => SttError::InternalError(to_string(&body).unwrap()),
        }
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
