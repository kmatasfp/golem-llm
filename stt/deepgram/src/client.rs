use std::{collections::HashMap, rc::Rc};

use golem_stt::{error::Error, http_client::HttpClient};
use log::trace;
use reqwest::Method;
use serde::Deserialize;

const BASE_URL: &str = "https://api.deepgram.com";

#[allow(non_camel_case_types)]
#[allow(unused)]
#[derive(Debug, Clone)]
pub enum AudioFormat {
    wav,
    mp3,
    flac,
    ogg,
    aac,
    pcm,
}

impl core::fmt::Display for AudioFormat {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string_representation = match self {
            AudioFormat::wav => "wav",
            AudioFormat::mp3 => "mp3",
            AudioFormat::flac => "flac",
            AudioFormat::ogg => "ogg",
            AudioFormat::aac => "aac",
            AudioFormat::pcm => "pcm",
        };
        write!(fmt, "{string_representation}")
    }
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub format: AudioFormat,
    pub channels: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub language: Option<String>,
    pub model: Option<String>,
    pub enable_profanity_filter: bool,
    pub enable_speaker_diarization: bool,
    pub keyterm: Option<String>, // only nova-3
}

/// The Deepgram Speech-to-Text API client for transcribing audio into the input language
///
/// https://developers.deepgram.com/reference/speech-to-text-api/listen
pub struct PreRecordedAudioApi<HC: HttpClient> {
    deepgram_api_token: Rc<str>,
    http_client: Rc<HC>,
}

#[allow(unused)]
impl<HC: HttpClient> PreRecordedAudioApi<HC> {
    pub fn new(deepgram_api_key: String, http_client: impl Into<Rc<HC>>) -> Self {
        Self {
            deepgram_api_token: Rc::from(format!("Token {}", deepgram_api_key)),
            http_client: http_client.into(),
        }
    }

    pub fn transcribe_audio(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionResponse, Error> {
        trace!("Sending request to OpenAI API: {request:?}");

        let mime_type = format!("audio/{}", request.audio_config.format);

        let audio_size_bytes = request.audio.len();

        let mut query: Vec<(&str, String)> = vec![];

        if let Some(channels) = request.audio_config.channels {
            if channels > 1 {
                query.push(("multichannel", "true".to_string()));
            }
        }

        if let Some(transcription_config) = request.transcription_config {
            if let Some(language) = transcription_config.language {
                query.push(("language", language));
            }

            if transcription_config.enable_profanity_filter {
                query.push(("profanity_filter", "true".to_string()));
            }

            if transcription_config.enable_speaker_diarization {
                query.push(("diarize", "true".to_string()));
            }

            if let Some(keyterm) = transcription_config.keyterm {
                query.push(("keyterm", keyterm));
            }

            if let Some(model) = transcription_config.model {
                query.push(("model", model));
            }
        }

        let req = self
            .http_client
            .request(Method::POST, format!("{}/v1/listen", BASE_URL))
            .header(reqwest::header::CONTENT_TYPE, mime_type)
            .header("Authorization", &*self.deepgram_api_token)
            .query(query.as_slice())
            .body(request.audio)
            .build()?;

        let response = self.http_client.execute(req)?;

        match response.status() {
            200 => {
                let deepgram_transcription: DeepgramTranscription = response.json()?;

                Ok(TranscriptionResponse {
                    audio_size_bytes,
                    deepgram_transcription,
                })
            }
            400 => Err(Error::APIBadRequest {
                provider_error: response.text()?,
            }),
            401 => Err(Error::APIUnauthorized {
                provider_error: response.text()?,
            }),
            402 => Err(Error::APIAccessDenied {
                provider_error: response.text()?,
            }),
            403 => Err(Error::APIForbidden {
                provider_error: response.text()?,
            }),
            status if status >= 500 => Err(Error::APIInternalServerError {
                provider_error: response.text()?,
            }),
            _ => Err(Error::APIUnknown {
                provider_error: response.text()?,
            }),
        }
    }
}

pub struct TranscriptionRequest {
    pub audio: Vec<u8>,
    pub audio_config: AudioConfig,
    pub transcription_config: Option<TranscriptionConfig>,
}

impl std::fmt::Debug for TranscriptionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptionRequest")
            .field("audio_size", &self.audio.len())
            .field("audio_config", &self.audio_config)
            .field("transcription_config", &self.transcription_config)
            .finish()
    }
}

#[allow(unused)]
#[derive(Debug, PartialEq)]
pub struct TranscriptionResponse {
    pub audio_size_bytes: usize,
    pub deepgram_transcription: DeepgramTranscription,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct DeepgramTranscription {
    pub metadata: Metadata,
    pub results: Results,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Metadata {
    pub transaction_key: String,
    pub request_id: String,
    pub sha256: String,
    pub created: String,
    pub duration: f32,
    pub channels: u8,
    pub models: Vec<String>,
    pub model_info: HashMap<String, ModelInfo>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub name: String,
    pub version: String,
    pub arch: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Results {
    pub channels: Vec<Channel>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Channel {
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Alternative {
    pub transcript: String,
    pub confidence: f32,
    pub words: Vec<Word>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Word {
    pub word: String,
    pub start: f32,
    pub end: f32,
    pub confidence: f32,
    pub speaker: Option<u8>,
    pub speaker_confidence: Option<f32>,
}
