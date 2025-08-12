use golem_stt::error::Error;
use golem_stt::http::WstdHttpClient;
use golem_stt::transcription::SttProviderClient;
use golem_stt::LOGGING_STATE;

use transcription::request::{
    AudioConfig, AudioFormat, DiarizationConfig, TranscriptionConfig, TranscriptionRequest,
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

mod transcription;

impl TryFrom<WitAudioFormat> for AudioFormat {
    type Error = WitSttError;

    fn try_from(wit_format: WitAudioFormat) -> Result<Self, Self::Error> {
        match wit_format {
            WitAudioFormat::Wav => Ok(AudioFormat::Wav),
            WitAudioFormat::Mp3 => Ok(AudioFormat::Mp3),
            WitAudioFormat::Flac => Ok(AudioFormat::Flac),
            WitAudioFormat::Ogg => Ok(AudioFormat::OggOpus),
            format => Err(WitSttError::UnsupportedFormat(format!(
                "{format:?} is not supported by Google Speech-to-Text"
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

        let language_codes = options.language.map(|lang| vec![lang]);

        let phrases: Vec<transcription::request::Phrase> = options
            .vocabulary
            .map(|vocab| {
                vocab
                    .phrases
                    .into_iter()
                    .map(|phrase| transcription::request::Phrase {
                        value: phrase.value,
                        boost: phrase.boost,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let diarization_config = if let Some(dc) = options.diarization {
            Some(transcription::request::DiarizationConfig {
                enabled: dc.enabled,
                min_speaker_count: dc.min_speaker_count.map(|count| count as i32).or(Some(1)), // set default value to 1
                max_speaker_count: dc.max_speaker_count.map(|count| count as i32),
            })
        } else {
            None
        };

        let enable_multi_channel = options.enable_multi_channel.unwrap_or(false);
        let enable_profanity_filter = options.profanity_filter.unwrap_or(false);

        Ok(transcription::request::TranscriptionConfig {
            language_codes,
            model: options.model,
            enable_profanity_filter,
            diarization: diarization_config,
            enable_multi_channel,
            phrases,
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
                sample_rate_hertz: request.config.sample_rate,
                channels: request.config.channels,
            },
            transcription_config,
        })
    }
}
