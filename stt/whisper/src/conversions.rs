use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError, TimingInfo as WitTimingInfo,
    TimingMarkType as WitTimingMarkType, TranscriptAlternative as WitTranscriptAlternative,
    TranscriptionMetadata as WitTranscriptionMetadata, WordSegment as WitWordSegment,
};

use golem_stt::golem::stt::transcription::{
    TranscribeOptions as WitTranscribeOptions, TranscriptionResult as WitTranscriptionResult,
};

use crate::client::{
    AudioFormat, TranscriptionConfig, TranscriptionResponse, WhisperTranscription,
};

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
            if crate::client::is_supported_language(language_code) {
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
