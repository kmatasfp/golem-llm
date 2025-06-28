use std::rc::Rc;

use derive_more::From;
use log::trace;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::languages::Language;

const BASE_URL: &str = "https://api.openai.com";

#[allow(unused)]
#[derive(Debug, From)]
pub enum Error {
    #[from]
    Reqwest(reqwest::Error),
    #[from]
    SerdeJson(serde_json::Error),
    APIBadRequest {
        body: ApiError,
    },
    APIUnauthorized {
        body: ApiError,
    },
    APIForbidden {
        body: ApiError,
    },
    APINotFound {
        body: ApiError,
    },
    APIConflict {
        body: ApiError,
    },
    APIUnprocessableEntity {
        body: ApiError,
    },
    APIRateLimit {
        body: ApiError,
    },
    #[allow(clippy::enum_variant_names)]
    APIInternalServerError {
        body: ApiError,
    },
    APIUnknown {
        body: ApiError,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[allow(non_camel_case_types)]
#[allow(unused)]
#[derive(Debug, Clone)]
pub enum AudioFormat {
    wav,
    mp3,
    flac,
    ogg,
}

impl core::fmt::Display for AudioFormat {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string_representation = match self {
            AudioFormat::wav => "wav",
            AudioFormat::mp3 => "mp3",
            AudioFormat::flac => "flac",
            AudioFormat::ogg => "ogg",
        };
        write!(fmt, "{string_representation}")
    }
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub format: AudioFormat,
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub language: Option<String>,
    pub enable_timestamps: bool,
    pub prompt: Option<String>,
}

/// The OpenAI API client for transcribing audio into the input language powered by their open source Whisper V2 model
///
/// https://platform.openai.com/docs/api-reference/audio/createTranscription
pub struct TranscriptionsApi {
    openai_api_token: Rc<str>,
    openai_api_base_url: Rc<str>,
    client: Client,
}

#[allow(unused)]
impl TranscriptionsApi {
    pub const SUPPORTED_LANGUAGES: [Language; 57] = [
        Language::new("af", "Afrikaans", "Afrikaans"),
        Language::new("ar", "Arabic", "العربية"),
        Language::new("hy", "Armenian", "հայերեն"),
        Language::new("az", "Azerbaijani", "azərbaycan dili"),
        Language::new("be", "Belarusian", "беларуская мова"),
        Language::new("bs", "Bosnian", "bosanski jezik"),
        Language::new("bg", "Bulgarian", "български език"),
        Language::new("ca", "Catalan", "català"),
        Language::new("zh", "Chinese", "中文"),
        Language::new("hr", "Croatian", "hrvatski jezik"),
        Language::new("cs", "Czech", "čeština"),
        Language::new("da", "Danish", "dansk"),
        Language::new("nl", "Dutch", "Nederlands"),
        Language::new("en", "English", "English"),
        Language::new("et", "Estonian", "eesti keel"),
        Language::new("fi", "Finnish", "suomi"),
        Language::new("fr", "French", "français"),
        Language::new("gl", "Galician", "galego"),
        Language::new("de", "German", "Deutsch"),
        Language::new("el", "Greek", "ελληνικά"),
        Language::new("he", "Hebrew", "עברית"),
        Language::new("hi", "Hindi", "हिन्दी"),
        Language::new("hu", "Hungarian", "magyar"),
        Language::new("is", "Icelandic", "íslenska"),
        Language::new("id", "Indonesian", "Bahasa Indonesia"),
        Language::new("it", "Italian", "italiano"),
        Language::new("ja", "Japanese", "日本語"),
        Language::new("kn", "Kannada", "ಕನ್ನಡ"),
        Language::new("kk", "Kazakh", "қазақ тілі"),
        Language::new("ko", "Korean", "한국어"),
        Language::new("lv", "Latvian", "latviešu valoda"),
        Language::new("lt", "Lithuanian", "lietuvių kalba"),
        Language::new("mk", "Macedonian", "македонски јазик"),
        Language::new("ms", "Malay", "Bahasa Melayu"),
        Language::new("mr", "Marathi", "मराठी"),
        Language::new("mi", "Maori", "te reo Māori"),
        Language::new("ne", "Nepali", "नेपाली"),
        Language::new("no", "Norwegian", "norsk"),
        Language::new("fa", "Persian", "فارسی"),
        Language::new("pl", "Polish", "polski"),
        Language::new("pt", "Portuguese", "português"),
        Language::new("ro", "Romanian", "română"),
        Language::new("ru", "Russian", "русский язык"),
        Language::new("sr", "Serbian", "српски језик"),
        Language::new("sk", "Slovak", "slovenčina"),
        Language::new("sl", "Slovenian", "slovenščina"),
        Language::new("es", "Spanish", "español"),
        Language::new("sw", "Swahili", "Kiswahili"),
        Language::new("sv", "Swedish", "svenska"),
        Language::new("tl", "Tagalog", "Tagalog"),
        Language::new("ta", "Tamil", "தமிழ்"),
        Language::new("th", "Thai", "ไทย"),
        Language::new("tr", "Turkish", "Türkçe"),
        Language::new("uk", "Ukrainian", "українська мова"),
        Language::new("ur", "Urdu", "اردو"),
        Language::new("vi", "Vietnamese", "Tiếng Việt"),
        Language::new("cy", "Welsh", "Cymraeg"),
    ];

    pub fn new(openai_api_key: String) -> Self {
        let client = Client::builder()
            .build()
            .expect("Failed to initialize HTTP client");
        Self {
            openai_api_token: Rc::from(format!("Bearer {}", openai_api_key)),
            openai_api_base_url: Rc::from(BASE_URL),
            client,
        }
    }

    pub fn get_supported_languages(&self) -> &[Language] {
        &Self::SUPPORTED_LANGUAGES
    }

    pub fn transcribe_audio(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionResponse, Error> {
        trace!("Sending request to OpenAI API: {request:?}");

        let file_name = format!("audio.{}", request.audio_config.format);
        let mime_type = format!("audio/{}", request.audio_config.format);

        let audio_size_bytes = request.audio.len();

        let part = reqwest::multipart::Part::bytes(request.audio)
            .file_name(file_name)
            .mime_str(&mime_type)?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", "whisper-1")
            .text("response_format", "verbose_json");

        if let Some(transcription_config) = request.transcription_config {
            if let Some(language) = transcription_config.language {
                form = form.text("language", language);
            }

            if transcription_config.enable_timestamps {
                form = form.text("", "timestamp_granularities[]=word");
            }

            if let Some(prompt) = transcription_config.prompt {
                form = form.text("prompt", prompt);
            }
        }

        let response = self
            .client
            .post(format!(
                "{}/v1/audio/transcriptions",
                self.openai_api_base_url
            ))
            .header("Authorization", &*self.openai_api_token)
            .multipart(form)
            .send()?;

        // match what official OpenAI SDK does https://github.com/openai/openai-python/blob/0673da62f2f2476a3e5791122e75ec0cbfd03442/src/openai/_client.py#L343
        match response.status() {
            reqwest::StatusCode::OK => {
                let response_body = response.text()?;
                trace!("Response body: {}", response_body);

                let whisper_transcription: WhisperTranscription =
                    serde_json::from_str(&response_body)?;

                Ok(TranscriptionResponse {
                    audio_size_bytes,
                    whisper_transcription,
                })
            }
            reqwest::StatusCode::BAD_REQUEST => Err(Error::APIBadRequest {
                body: response.json()?,
            }),
            reqwest::StatusCode::UNAUTHORIZED => Err(Error::APIUnauthorized {
                body: response.json()?,
            }),
            reqwest::StatusCode::FORBIDDEN => Err(Error::APIForbidden {
                body: response.json()?,
            }),
            reqwest::StatusCode::NOT_FOUND => Err(Error::APINotFound {
                body: response.json()?,
            }),
            reqwest::StatusCode::CONFLICT => Err(Error::APIConflict {
                body: response.json()?,
            }),
            reqwest::StatusCode::UNPROCESSABLE_ENTITY => Err(Error::APIUnprocessableEntity {
                body: response.json()?,
            }),
            reqwest::StatusCode::TOO_MANY_REQUESTS => Err(Error::APIRateLimit {
                body: response.json()?,
            }),
            status if status.is_server_error() => Err(Error::APIInternalServerError {
                body: response.json()?,
            }),
            _ => Err(Error::APIUnknown {
                body: response.json()?,
            }),
        }
    }
}

#[derive(Clone)]
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
pub struct TranscriptionResponse {
    pub audio_size_bytes: usize,
    pub whisper_transcription: WhisperTranscription,
}

#[allow(unused)]
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum WhisperTranscription {
    Segments {
        task: String,
        language: String,
        duration: f64,
        text: String,
        segments: Vec<Segment>,
        usage: Usage,
    },
    Words {
        task: String,
        language: String,
        duration: f64,
        text: String,
        words: Vec<Word>,
        usage: Usage,
    },
}

#[allow(unused)]
#[derive(Debug, Deserialize, PartialEq)]
pub struct Word {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

#[allow(unused)]
#[derive(Debug, Deserialize, PartialEq)]
pub struct Segment {
    pub id: u32,
    pub seek: u32,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub temperature: f64,
    pub avg_logprob: f64,
    pub compression_ratio: f64,
    pub no_speech_prob: f64,
}

#[allow(unused)]
#[derive(Debug, Deserialize, PartialEq)]
pub struct Usage {
    pub r#type: String,
    pub seconds: u32,
}

#[allow(unused)]
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ApiError {
    pub error: ErrorBody,
}

#[allow(unused)]
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorBody {
    pub message: String,
    pub r#type: String,
    pub param: Option<String>,
    pub code: Option<String>,
}
