use std::rc::Rc;

use bytes::Bytes;
use derive_more::From;
use log::trace;
use reqwest::{Client, IntoUrl, Method, Request, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::languages::Language;

const BASE_URL: &str = "https://api.openai.com";

pub const WHISPER_SUPPORTED_LANGUAGES: [Language; 57] = [
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

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Bytes,
}

#[allow(unused)]
impl HttpResponse {
    pub fn new(status: u16, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn bytes(self) -> Bytes {
        self.body
    }

    pub fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }
}

pub trait HttpClient {
    fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder;
    fn execute(&self, request: Request) -> Result<HttpResponse, Error>;
}

pub struct ReqwestHttpClient {
    client: Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .build()
            .expect("Failed to initialize HTTP client");
        Self { client }
    }
}

impl HttpClient for ReqwestHttpClient {
    fn execute(&self, request: Request) -> Result<HttpResponse, Error> {
        let response = self.client.execute(request)?;
        let status = response.status().as_u16();
        let body = response.bytes()?;
        Ok(HttpResponse { status, body })
    }

    fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        self.client.request(method, url)
    }
}

/// The OpenAI API client for transcribing audio into the input language powered by their open source Whisper V2 model
///
/// https://platform.openai.com/docs/api-reference/audio/createTranscription
pub struct TranscriptionsApi<HC: HttpClient> {
    openai_api_token: Rc<str>,
    openai_api_base_url: Rc<str>,
    http_client: Rc<HC>,
}

#[allow(unused)]
impl<HC: HttpClient> TranscriptionsApi<HC> {
    pub fn new(openai_api_key: String, http_client: impl Into<Rc<HC>>) -> Self {
        Self {
            openai_api_token: Rc::from(format!("Bearer {}", openai_api_key)),
            openai_api_base_url: Rc::from(BASE_URL),
            http_client: http_client.into(),
        }
    }

    pub fn get_supported_languages(&self) -> &[Language] {
        &WHISPER_SUPPORTED_LANGUAGES
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

        let req = self
            .http_client
            .request(
                Method::POST,
                format!("{}/v1/audio/transcriptions", self.openai_api_base_url),
            )
            .header("Authorization", &*self.openai_api_token)
            .multipart(form)
            .build()?;

        let response = self.http_client.execute(req)?;

        // match what official OpenAI SDK does https://github.com/openai/openai-python/blob/0673da62f2f2476a3e5791122e75ec0cbfd03442/src/openai/_client.py#L343
        match response.status() {
            200 => {
                let whisper_transcription: WhisperTranscription = response.json()?;

                Ok(TranscriptionResponse {
                    audio_size_bytes,
                    whisper_transcription,
                })
            }
            400 => Err(Error::APIBadRequest {
                body: response.json()?,
            }),
            401 => Err(Error::APIUnauthorized {
                body: response.json()?,
            }),
            403 => Err(Error::APIForbidden {
                body: response.json()?,
            }),
            404 => Err(Error::APINotFound {
                body: response.json()?,
            }),
            409 => Err(Error::APIConflict {
                body: response.json()?,
            }),
            422 => Err(Error::APIUnprocessableEntity {
                body: response.json()?,
            }),
            429 => Err(Error::APIRateLimit {
                body: response.json()?,
            }),
            status if status >= 500 => Err(Error::APIInternalServerError {
                body: response.json()?,
            }),
            _ => Err(Error::APIUnknown {
                body: response.json()?,
            }),
        }
    }
}

impl TranscriptionsApi<ReqwestHttpClient> {
    pub fn live(openai_api_key: String) -> Self {
        Self::new(openai_api_key, ReqwestHttpClient::new())
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

#[cfg(test)]
mod tests {
    use std::cell::Ref;
    use std::collections::HashMap;
    use std::io::{Cursor, Read};
    use std::rc::Rc;

    use multipart::server::Multipart;

    use super::*;

    const TEST_API_KEY: &str = "test-api-key";

    struct MockHttpClient {
        pub responses: std::cell::RefCell<std::collections::VecDeque<Result<HttpResponse, Error>>>,
        pub captured_requests: std::cell::RefCell<Vec<reqwest::Request>>,
    }

    #[allow(unused)]
    impl MockHttpClient {
        pub fn new() -> Self {
            Self {
                responses: std::cell::RefCell::new(std::collections::VecDeque::new()),
                captured_requests: std::cell::RefCell::new(Vec::new()),
            }
        }

        pub fn expect_response(&self, response: HttpResponse) {
            self.responses.borrow_mut().push_back(Ok(response));
        }

        pub fn get_captured_requests(&self) -> Ref<Vec<reqwest::Request>> {
            self.captured_requests.borrow()
        }

        pub fn clear_captured_requests(&self) {
            self.captured_requests.borrow_mut().clear();
        }

        pub fn captured_request_count(&self) -> usize {
            self.captured_requests.borrow().len()
        }

        pub fn last_captured_request(&self) -> Option<Ref<reqwest::Request>> {
            let borrow = self.captured_requests.borrow();
            if borrow.is_empty() {
                None
            } else {
                Some(Ref::map(borrow, |requests| requests.last().unwrap()))
            }
        }
    }

    impl HttpClient for MockHttpClient {
        fn execute(&self, mut request: Request) -> Result<HttpResponse, Error> {
            // consume the body so we can verify it later
            request.body_mut().as_mut().unwrap().buffer().unwrap();

            self.captured_requests.borrow_mut().push(request);
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(Error::APIUnknown {
                    body: ApiError {
                        error: ErrorBody {
                            message: "unexpected error".to_string(),
                            r#type: "unknown".to_string(),
                            param: None,
                            code: None,
                        },
                    },
                }))
        }

        fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
            Client::builder()
                .build()
                .expect("Failed to initialize HTTP client")
                .request(method, url)
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct MultipartField {
        pub data: Vec<u8>,
        pub filename: Option<String>,
        pub content_type: Option<String>,
    }

    #[test]
    fn test_api_key_gets_passed_as_auth_header() {
        let response_body = r#"
            {
                "task": "transcribe",
                "language": "en",
                "duration": 10.5,
                "text": "Hello,!",
                "segments": [
                    {
                        "id": 0,
                        "seek": 0,
                        "start": 0.0,
                        "end": 2.5,
                        "text": "Hello,",
                        "temperature": 0.0,
                        "avg_logprob": -0.5,
                        "compression_ratio": 1.0,
                        "no_speech_prob": 0.1
                    }
                ],
                "usage": {
                    "type": "transcribe",
                    "seconds": 10
                }
            }
        "#;

        let mock_client = Rc::new(MockHttpClient::new());
        mock_client.expect_response(HttpResponse::new(200, response_body));

        let api: TranscriptionsApi<MockHttpClient> =
            TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client.clone());

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes.clone(),
            audio_config: AudioConfig {
                format: AudioFormat::mp3,
            },
            transcription_config: None,
        };

        api.transcribe_audio(request).unwrap();

        let captured_request = mock_client.last_captured_request().unwrap();

        assert_eq!(captured_request.method(), &Method::POST);
        assert!(captured_request
            .url()
            .path()
            .contains("/v1/audio/transcriptions"));

        assert!(captured_request
            .headers()
            .iter()
            .find(|(name, value)| *name == "authorization"
                && value.to_str().unwrap() == format!("Bearer {}", TEST_API_KEY))
            .is_some());

        assert_eq!(mock_client.captured_request_count(), 1);
    }

    #[test]
    fn test_resquest_gets_sent_as_multi_part_form_data() {
        let response_body = r#"
            {
                "task": "transcribe",
                "language": "en",
                "duration": 10.5,
                "text": "Hello,!",
                "segments": [
                    {
                        "id": 0,
                        "seek": 0,
                        "start": 0.0,
                        "end": 2.5,
                        "text": "Hello,",
                        "temperature": 0.0,
                        "avg_logprob": -0.5,
                        "compression_ratio": 1.0,
                        "no_speech_prob": 0.1
                    }
                ],
                "usage": {
                    "type": "transcribe",
                    "seconds": 10
                }
            }
        "#;

        let mock_client = Rc::new(MockHttpClient::new());
        mock_client.expect_response(HttpResponse::new(200, response_body));

        let api: TranscriptionsApi<MockHttpClient> =
            TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client.clone());

        let audio_bytes = b"fake audio data".to_vec();

        let language = "en".to_string();
        let prompt = "foo".to_string();

        let request = TranscriptionRequest {
            audio: audio_bytes.clone(),
            audio_config: AudioConfig {
                format: AudioFormat::mp3,
            },
            transcription_config: Some(TranscriptionConfig {
                language: Some(language.clone()),
                enable_timestamps: false,
                prompt: Some(prompt.clone()),
            }),
        };

        api.transcribe_audio(request).unwrap();

        let captured_request = mock_client.last_captured_request().unwrap();

        let content_type_header = captured_request
            .headers()
            .get("Content-Type")
            .unwrap()
            .to_str()
            .unwrap();

        assert!(
            content_type_header.starts_with("multipart/form-data"),
            "should be multipart/form-data"
        );

        let boundary = content_type_header.split("boundary=").nth(1).unwrap();

        let body_bytes = captured_request.body().unwrap().as_bytes().unwrap();

        let cursor = Cursor::new(body_bytes);
        let mut multipart = Multipart::with_body(cursor, boundary);
        let mut fields: HashMap<String, MultipartField> = HashMap::new();

        while let Ok(Some(mut field)) = multipart.read_entry() {
            let field_name = field.headers.name.clone();

            let mut data = Vec::new();
            field.data.read_to_end(&mut data).unwrap();

            let multipart_field = MultipartField {
                data,
                filename: field.headers.filename.clone(),
                content_type: field.headers.content_type.as_ref().map(|ct| ct.to_string()),
            };

            fields.insert(field_name.to_string(), multipart_field);
        }

        let file_field = fields.get("file").unwrap();
        assert_eq!(
            file_field,
            &MultipartField {
                data: audio_bytes,
                filename: Some("audio.mp3".to_string()),
                content_type: Some("audio/mp3".to_string()),
            }
        );

        let model_field = fields.get("model").unwrap();
        assert_eq!(
            model_field,
            &MultipartField {
                data: b"whisper-1".to_vec(),
                filename: None,
                content_type: None,
            }
        );

        let response_format_field = fields.get("response_format").unwrap();
        assert_eq!(
            response_format_field,
            &MultipartField {
                data: b"verbose_json".to_vec(),
                filename: None,
                content_type: None,
            }
        );

        let language_field = fields.get("language").unwrap();
        assert_eq!(
            language_field,
            &MultipartField {
                data: language.as_bytes().to_vec(),
                filename: None,
                content_type: None,
            }
        );

        let verbose_field = fields.get("prompt").unwrap();
        assert_eq!(
            verbose_field,
            &MultipartField {
                data: prompt.as_bytes().to_vec(),
                filename: None,
                content_type: None,
            }
        );

        // Verify that we captured exactly one request
        assert_eq!(mock_client.captured_request_count(), 1);
    }

    #[test]
    fn test_word_level_timestamps_requested() {
        let response_body = r#"
               {
                   "task": "transcribe",
                   "language": "en",
                   "duration": 8.2,
                   "text": "Hello world",
                   "words": [
                       {
                           "word": "Hello",
                           "start": 0.0,
                           "end": 1.5
                       },
                       {
                           "word": "world",
                           "start": 1.5,
                           "end": 3.0
                       }
                   ],
                   "usage": {
                       "type": "transcribe",
                       "seconds": 10
                   }
               }
           "#;

        let mock_client = Rc::new(MockHttpClient::new());
        mock_client.expect_response(HttpResponse::new(200, response_body));

        let api: TranscriptionsApi<MockHttpClient> =
            TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client.clone());

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes.clone(),
            audio_config: AudioConfig {
                format: AudioFormat::mp3,
            },
            transcription_config: Some(TranscriptionConfig {
                language: None,
                enable_timestamps: true,
                prompt: None,
            }),
        };

        api.transcribe_audio(request).unwrap();

        let captured_request = mock_client.last_captured_request().unwrap();

        let content_type_header = captured_request
            .headers()
            .get("Content-Type")
            .unwrap()
            .to_str()
            .unwrap();

        assert!(
            content_type_header.starts_with("multipart/form-data"),
            "should be multipart/form-data"
        );

        let boundary = content_type_header.split("boundary=").nth(1).unwrap();

        let body_bytes = captured_request.body().unwrap().as_bytes().unwrap();

        let cursor = Cursor::new(body_bytes);
        let mut multipart = Multipart::with_body(cursor, boundary);
        let mut fields: HashMap<String, MultipartField> = HashMap::new();

        while let Ok(Some(mut field)) = multipart.read_entry() {
            let field_name = field.headers.name.clone();

            println!("Field name: {}", field_name);

            let mut data = Vec::new();
            field.data.read_to_end(&mut data).unwrap();

            let multipart_field = MultipartField {
                data,
                filename: field.headers.filename.clone(),
                content_type: field.headers.content_type.as_ref().map(|ct| ct.to_string()),
            };

            fields.insert(field_name.to_string(), multipart_field);
        }

        let timestamp_granularity_field = fields.get("").unwrap();
        assert_eq!(
            timestamp_granularity_field,
            &MultipartField {
                data: b"timestamp_granularities[]=word".to_vec(),
                filename: None,
                content_type: None,
            }
        );

        // Verify that we captured exactly one request
        assert_eq!(mock_client.captured_request_count(), 1);
    }

    #[test]
    fn test_transcribe_audio_success_words() {
        let response_body = r#"
               {
                   "task": "transcribe",
                   "language": "en",
                   "duration": 8.2,
                   "text": "Hello world",
                   "words": [
                       {
                           "word": "Hello",
                           "start": 0.0,
                           "end": 1.5
                       },
                       {
                           "word": "world",
                           "start": 1.5,
                           "end": 3.0
                       }
                   ],
                   "usage": {
                       "type": "transcribe",
                       "seconds": 10
                   }
               }
           "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(200, response_body));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake audio data for words test".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes.clone(),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
            },
            transcription_config: Some(TranscriptionConfig {
                language: Some("en".to_string()),
                enable_timestamps: true,
                prompt: None,
            }),
        };

        let response = api.transcribe_audio(request).unwrap();

        let expected_response = TranscriptionResponse {
            audio_size_bytes: audio_bytes.len(),
            whisper_transcription: WhisperTranscription::Words {
                task: "transcribe".to_string(),
                language: "en".to_string(),
                duration: 8.2,
                text: "Hello world".to_string(),
                words: vec![
                    Word {
                        word: "Hello".to_string(),
                        start: 0.0,
                        end: 1.5,
                    },
                    Word {
                        word: "world".to_string(),
                        start: 1.5,
                        end: 3.0,
                    },
                ],
                usage: Usage {
                    r#type: "transcribe".to_string(),
                    seconds: 10,
                },
            },
        };

        assert_eq!(response, expected_response);
    }

    #[test]
    fn test_transcribe_audio_error_bad_request() {
        let error_response = r#"
                {
                    "error": {
                        "message": "[{'type': 'enum', 'loc': ('body', 'timestamp_granularities[]', 0), 'msg': \"Input should be 'segment' or 'word'\", 'input': 'word,segments', 'ctx': {'expected': \"'segment' or 'word'\"}}]",
                        "type": "invalid_request_error",
                        "param": null,
                        "code": null
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(400, error_response));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::mp3,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APIBadRequest { body } => {
                assert_eq!(body.error.message, "[{'type': 'enum', 'loc': ('body', 'timestamp_granularities[]', 0), 'msg': \"Input should be 'segment' or 'word'\", 'input': 'word,segments', 'ctx': {'expected': \"'segment' or 'word'\"}}]");
                assert_eq!(body.error.r#type, "invalid_request_error");
                assert_eq!(body.error.param, None);
                assert_eq!(body.error.code, None);
            }
            _ => panic!("Expected APIBadRequest error"),
        }
    }

    #[test]
    fn test_transcribe_audio_error_unauthorized() {
        let error_response = r#"
                {
                    "error": {
                        "message": "Incorrect API key provided",
                        "type": "invalid_request_error",
                        "param": null,
                        "code": "invalid_api_key"
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(401, error_response));

        let api = TranscriptionsApi::new("invalid_key".to_string(), mock_client);

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::wav,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APIUnauthorized { body } => {
                assert_eq!(body.error.message, "Incorrect API key provided");
                assert_eq!(body.error.r#type, "invalid_request_error");
                assert_eq!(body.error.param, None);
                assert_eq!(body.error.code, Some("invalid_api_key".to_string()));
            }
            _ => panic!("Expected APIUnauthorized error"),
        }
    }

    #[test]
    fn test_transcribe_audio_error_forbidden() {
        let error_response = r#"
                {
                    "error": {
                        "message": "Your account does not have access to this resource",
                        "type": "insufficient_quota",
                        "param": null,
                        "code": "insufficient_quota"
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(403, error_response));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::flac,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APIForbidden { body } => {
                assert_eq!(
                    body.error.message,
                    "Your account does not have access to this resource"
                );
                assert_eq!(body.error.r#type, "insufficient_quota");
                assert_eq!(body.error.param, None);
                assert_eq!(body.error.code, Some("insufficient_quota".to_string()));
            }
            _ => panic!("Expected APIForbidden error"),
        }
    }

    #[test]
    fn test_transcribe_audio_error_not_found() {
        let error_response = r#"
                {
                    "error": {
                        "message": "The model 'xxxxxxx-2' does not exist",
                        "type": "invalid_request_error",
                        "param": "model",
                        "code": null
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(404, error_response));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::ogg,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APINotFound { body } => {
                assert_eq!(body.error.message, "The model 'xxxxxxx-2' does not exist");
                assert_eq!(body.error.r#type, "invalid_request_error");
                assert_eq!(body.error.param, Some("model".to_string()));
                assert_eq!(body.error.code, None);
            }
            _ => panic!("Expected APINotFound error"),
        }
    }

    #[test]
    fn test_transcribe_audio_error_unprocessable_entity() {
        let error_response = r#"
                {
                    "error": {
                        "message": "The audio file is too large. Maximum size is 25MB.",
                        "type": "invalid_request_error",
                        "param": "file",
                        "code": "file_too_large"
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(422, error_response));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake large audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::mp3,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APIUnprocessableEntity { body } => {
                assert_eq!(
                    body.error.message,
                    "The audio file is too large. Maximum size is 25MB."
                );
                assert_eq!(body.error.r#type, "invalid_request_error");
                assert_eq!(body.error.param, Some("file".to_string()));
                assert_eq!(body.error.code, Some("file_too_large".to_string()));
            }
            _ => panic!("Expected APIUnprocessableEntity error"),
        }
    }

    #[test]
    fn test_transcribe_audio_error_rate_limit() {
        let error_response = r#"
                {
                    "error": {
                        "message": "Rate limit exceeded. Please try again later.",
                        "type": "requests",
                        "param": null,
                        "code": "rate_limit_exceeded"
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(429, error_response));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::wav,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APIRateLimit { body } => {
                assert_eq!(
                    body.error.message,
                    "Rate limit exceeded. Please try again later."
                );
                assert_eq!(body.error.r#type, "requests");
                assert_eq!(body.error.param, None);
                assert_eq!(body.error.code, Some("rate_limit_exceeded".to_string()));
            }
            _ => panic!("Expected APIRateLimit error"),
        }
    }

    #[test]
    fn test_transcribe_audio_error_internal_server_error() {
        let error_response = r#"
                {
                    "error": {
                        "message": "The server encountered an internal error and was unable to complete your request.",
                        "type": "server_error",
                        "param": null,
                        "code": null
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(500, error_response));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::mp3,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APIInternalServerError { body } => {
                assert_eq!(body.error.message, "The server encountered an internal error and was unable to complete your request.");
                assert_eq!(body.error.r#type, "server_error");
                assert_eq!(body.error.param, None);
                assert_eq!(body.error.code, None);
            }
            _ => panic!("Expected APIInternalServerError error"),
        }
    }

    #[test]
    fn test_transcribe_audio_error_unknown_status() {
        let error_response = r#"
                {
                    "error": {
                        "message": "Unknown error occurred",
                        "type": "unknown_error",
                        "param": null,
                        "code": "unknown"
                    }
                }
            "#;

        let mock_client = MockHttpClient::new();
        mock_client.expect_response(HttpResponse::new(418, error_response));

        let api = TranscriptionsApi::new(TEST_API_KEY.to_string(), mock_client);

        let audio_bytes = b"fake audio data".to_vec();

        let request = TranscriptionRequest {
            audio: audio_bytes,
            audio_config: AudioConfig {
                format: AudioFormat::flac,
            },
            transcription_config: None,
        };

        let result = api.transcribe_audio(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::APIUnknown { body } => {
                assert_eq!(body.error.message, "Unknown error occurred");
                assert_eq!(body.error.r#type, "unknown_error");
                assert_eq!(body.error.param, None);
                assert_eq!(body.error.code, Some("unknown".to_string()));
            }
            _ => panic!("Expected APIUnknown error"),
        }
    }
}
