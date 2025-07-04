use golem_stt::golem::stt::types::{
    AudioFormat as WitAudioFormat, SttError, TimingInfo as WitTimingInfo,
    TimingMarkType as WitTimingMarkType, TranscriptAlternative as WitTranscriptAlternative,
    TranscriptionMetadata as WitTranscriptionMetadata, WordSegment as WitWordSegment,
};

use golem_stt::golem::stt::transcription::{
    Phrase as WitPhrase, TranscribeOptions as WitTranscribeOptions,
    TranscriptionRequest as WitTranscriptionRequest, TranscriptionResult as WitTranscriptionResult,
    Vocabulary as WitVocabulary,
};

use crate::client::{
    AudioConfig, AudioFormat, Keyword, TranscriptionConfig, TranscriptionRequest,
    TranscriptionResponse,
};

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
    type Error = SttError;

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
            if crate::client::is_supported_language(language_code) {
                return Err(SttError::UnsupportedLanguage(language_code.to_owned()));
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

        Ok(TranscriptionConfig {
            language: options.language,
            model: options.model,
            enable_profanity_filter: options.profanity_filter.unwrap_or(false),
            enable_speaker_diarization: options.enable_speaker_diarization.unwrap_or(false),
            keywords,
            keyterms,
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
            duration_seconds: response.deepgram_transcription.metadata.duration as f32,
            audio_size_bytes: response.audio_size_bytes as u32,
            request_id: response.deepgram_transcription.metadata.request_id,
            model: serde_json::to_string(&response.deepgram_transcription.metadata.model_info).ok(),
            language: response.language,
        };

        let alternatives: Vec<WitTranscriptAlternative> = response
            .deepgram_transcription
            .results
            .channels
            .into_iter()
            .flat_map(|channel| {
                channel.alternatives.into_iter().map(|alternative| {
                    let words: Vec<WitWordSegment> = alternative
                        .words
                        .into_iter()
                        .map(|word| WitWordSegment {
                            text: word.word,
                            timing_info: Some(WitTimingInfo {
                                start_time_seconds: word.start,
                                end_time_seconds: word.end,
                                mark_type: WitTimingMarkType::Word,
                            }),
                            confidence: Some(word.confidence),
                            speaker_id: word.speaker.map(|id| id.to_string()),
                        })
                        .collect();

                    WitTranscriptAlternative {
                        text: alternative.transcript,
                        confidence: alternative.confidence,
                        words,
                    }
                })
            })
            .collect();

        WitTranscriptionResult {
            metadata,
            alternatives,
        }
    }
}
