use bytes::Bytes;
use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError, TimingInfo as WitTimingInfo,
    TimingMarkType as WitTimingMarkType, TranscriptAlternative as WitTranscriptAlternative,
    TranscriptionMetadata as WitTranscriptionMetadata, WordSegment as WitWordSegment,
};

use golem_stt::golem::stt::transcription::{
    TranscribeOptions as WitTranscribeOptions, TranscriptionRequest as WitTranscriptionRequest,
    TranscriptionResult as WitTranscriptionResult,
};

use crate::client::{
    is_supported_language, AudioConfig, AudioFormat, TranscriptionConfig, TranscriptionRequest,
    TranscriptionResponse,
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
        if let Some(language_code) = &options.language {
            if !is_supported_language(language_code) {
                return Err(SttError::UnsupportedLanguage(language_code.clone()));
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
    type Error = SttError;

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
            audio: Bytes::from(audio),
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
