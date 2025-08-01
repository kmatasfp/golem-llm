use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use golem_stt::error::Error as SttError;
use golem_stt::http::HttpClient;
use golem_stt::runtime::AsyncRuntime;
use http::{header::CONTENT_TYPE, Method, Request, StatusCode};
use log::trace;
use serde::{Deserialize, Serialize};

use super::gcp_auth::GcpAuth;

const BASE_URL: &str = "https://speech.googleapis.com/v2";

#[derive(Debug, Clone)]
pub enum AudioFormat {
    LinearPcm,
    Flac,
    Mp3,
    OggOpus,
    WebmOpus,
    AmrNb,
    AmrWb,
    Wav,
    Mp4,
    M4a,
    Mov,
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioFormat::LinearPcm => write!(f, "LINEAR16"),
            AudioFormat::Flac => write!(f, "FLAC"),
            AudioFormat::Mp3 => write!(f, "MP3"),
            AudioFormat::OggOpus => write!(f, "OGG_OPUS"),
            AudioFormat::WebmOpus => write!(f, "WEBM_OPUS"),
            AudioFormat::AmrNb => write!(f, "AMR"),
            AudioFormat::AmrWb => write!(f, "AMR_WB"),
            AudioFormat::Wav => write!(f, "WAV_LINEAR16"),
            AudioFormat::Mp4 => write!(f, "MP4_AAC"),
            AudioFormat::M4a => write!(f, "M4A_AAC"),
            AudioFormat::Mov => write!(f, "MOV_AAC"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub format: AudioFormat,
    pub sample_rate_hertz: Option<u32>,
    pub channels: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct Phrase {
    pub value: String,
    pub boost: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct DiarizationConfig {
    pub enabled: bool,
    pub min_speaker_count: Option<i32>,
    pub max_speaker_count: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub language_codes: Option<Vec<String>>,
    pub model: Option<String>,
    pub enable_profanity_filter: bool,
    pub diarization: Option<DiarizationConfig>,
    pub enable_multi_channel: bool,
    pub phrases: Vec<Phrase>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct StartBatchRecognizeRequest {
    pub config: RecognitionConfig,
    pub config_mask: Option<String>,
    pub files: Vec<BatchRecognizeFileMetadata>,
    pub recognition_output_config: RecognitionOutputConfig,
    pub processing_strategy: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RecognitionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language_codes: Option<Vec<String>>,
    features: RecognitionFeatures,
    #[serde(skip_serializing_if = "Option::is_none")]
    adaptation: Option<SpeechAdaptation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_decoding_config: Option<AutoDetectDecodingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    explicit_decoding_config: Option<ExplicitDecodingConfig>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RecognitionFeatures {
    #[serde(skip_serializing_if = "Option::is_none")]
    profanity_filter: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_word_time_offsets: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_word_confidence: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_automatic_punctuation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    multi_channel_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diarization_config: Option<SpeakerDiarizationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_alternatives: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetBatchRecognizeOperationResponse {
    pub name: String,
    pub metadata: Option<OperationMetadata>,
    pub done: bool,
    pub error: Option<OperationError>,
    pub response: Option<BatchRecognizeResponse>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchRecognizeResponse {
    pub results: HashMap<String, BatchRecognizeFileResult>,
    pub total_billed_duration: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchRecognizeFileResult {
    pub error: Option<OperationError>,
    pub metadata: Option<RecognitionResponseMetadata>,
    pub inline_result: Option<InlineResult>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchRecognizeResults {
    pub results: Vec<SpeechRecognitionResult>,
    pub metadata: Option<RecognitionResponseMetadata>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InlineResult {
    pub transcript: BatchRecognizeResults,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct AutoDetectDecodingConfig {}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ExplicitDecodingConfig {
    encoding: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_rate_hertz: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_channel_count: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SpeakerDiarizationConfig {
    min_speaker_count: i32,
    max_speaker_count: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SpeechAdaptation {
    phrase_sets: Vec<AdaptationPhraseSet>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AdaptationPhraseSet {
    inline_phrase_set: PhraseSet,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PhraseSet {
    pub phrases: Vec<PhraseItem>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PhraseItem {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BatchRecognizeFileMetadata {
    uri: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RecognitionOutputConfig {
    inline_response_config: InlineOutputConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct InlineOutputConfig {}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OperationMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    create_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    update_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress_percent: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    code: i32,
    message: String,
    #[serde(default)]
    details: Vec<serde_json::Value>,
}

// Response structs
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecognitionResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_billed_duration: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpeechRecognitionResult {
    pub alternatives: Vec<SpeechRecognitionAlternative>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_tag: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_end_offset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpeechRecognitionAlternative {
    pub transcript: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub words: Vec<WordInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WordInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<String>,
    pub word: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_label: Option<String>,
}

pub trait SpeechToTextService {
    async fn start_batch_recognize(
        &mut self,
        operation_name: &str,
        audio_gcs_uris: Vec<String>,
        audio_config: &AudioConfig,
        transcription_config: Option<&TranscriptionConfig>,
    ) -> Result<GetBatchRecognizeOperationResponse, SttError>;

    async fn get_batch_recognize(
        &mut self,
        request_id: &str,
        operation_name: &str,
    ) -> Result<GetBatchRecognizeOperationResponse, SttError>;

    async fn wait_for_batch_recognize_completion(
        &mut self,
        request_id: &str,
        operation_name: &str,
        max_wait_time: Duration,
    ) -> Result<GetBatchRecognizeOperationResponse, SttError>;

    async fn delete_batch_recognize(
        &mut self,
        request_id: &str,
        operation_name: &str,
    ) -> Result<(), SttError>;
}

pub struct SpeechToTextClient<HC: HttpClient, RT: AsyncRuntime> {
    http_client: Rc<HC>,
    auth: GcpAuth<HC>,
    location: String,
    runtime: RT,
}

impl<HC: HttpClient, RT: AsyncRuntime> SpeechToTextClient<HC, RT> {
    pub fn new(
        auth: GcpAuth<HC>,
        http_client: Rc<HC>,
        location: Option<String>,
        runtime: RT,
    ) -> Self {
        Self {
            http_client,
            auth,
            location: location.unwrap_or_else(|| "global".to_string()),
            runtime,
        }
    }
    async fn make_authenticated_request<T>(
        &mut self,
        uri: &str,
        request_id: &str,
        method: Method,
        body: Option<Vec<u8>>,
    ) -> Result<T, SttError>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let access_token = self
            .auth
            .get_access_token()
            .await
            .map_err(|e| SttError::AuthError(format!("Failed to get access token: {:?}", e)))?;

        let mut request_builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("Authorization", format!("Bearer {}", access_token));

        if body.is_some() {
            request_builder = request_builder.header(CONTENT_TYPE, "application/json");
        }

        let http_request = request_builder
            .body(body.unwrap_or_default())
            .map_err(|e| (request_id.to_string(), golem_stt::http::Error::HttpError(e)))?;

        trace!("Sending request to GCP Speech-to-Text API: {}", uri);

        let response = self
            .http_client
            .execute(http_request.clone())
            .await
            .map_err(|e| SttError::Http(request_id.to_string(), e))?;

        if response.status().is_success() {
            let json_response: T = serde_json::from_slice(response.body()).map_err(|e| {
                (
                    request_id.to_string(),
                    golem_stt::http::Error::Generic(format!(
                        "Failed to deserialize response: {}",
                        e
                    )),
                )
            })?;

            Ok(json_response)
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|_| "Unknown error".to_string());

            let status = response.status();
            let request_id = request_id.to_string();

            match status {
                StatusCode::BAD_REQUEST => Err(SttError::APIBadRequest {
                    request_id,
                    provider_error: error_body,
                }),
                StatusCode::UNAUTHORIZED => Err(SttError::APIUnauthorized {
                    request_id,
                    provider_error: error_body,
                }),
                StatusCode::FORBIDDEN => Err(SttError::APIForbidden {
                    request_id,
                    provider_error: error_body,
                }),
                StatusCode::NOT_FOUND => Err(SttError::APINotFound {
                    request_id,
                    provider_error: error_body,
                }),
                StatusCode::TOO_MANY_REQUESTS => Err(SttError::APIRateLimit {
                    request_id,
                    provider_error: error_body,
                }),
                s if s.is_server_error() => Err(SttError::APIInternalServerError {
                    request_id,
                    provider_error: error_body,
                }),
                _ => Err(SttError::APIUnknown {
                    request_id,
                    provider_error: error_body,
                }),
            }
        }
    }
}

impl<HC: HttpClient, RT: AsyncRuntime> SpeechToTextService for SpeechToTextClient<HC, RT> {
    async fn start_batch_recognize(
        &mut self,
        request_id: &str,
        audio_gcs_uris: Vec<String>,
        audio_config: &AudioConfig,
        transcription_config: Option<&TranscriptionConfig>,
    ) -> Result<GetBatchRecognizeOperationResponse, SttError> {
        let (auto_decoding_config, explicit_decoding_config) = match audio_config.format {
            AudioFormat::Wav
            | AudioFormat::Flac
            | AudioFormat::Mp3
            | AudioFormat::OggOpus
            | AudioFormat::WebmOpus
            | AudioFormat::AmrNb
            | AudioFormat::AmrWb
            | AudioFormat::Mp4
            | AudioFormat::M4a
            | AudioFormat::Mov => (Some(AutoDetectDecodingConfig {}), None),
            AudioFormat::LinearPcm => (
                None,
                Some(ExplicitDecodingConfig {
                    encoding: "LINEAR16".to_string(),
                    sample_rate_hertz: audio_config.sample_rate_hertz,
                    audio_channel_count: audio_config.channels,
                }),
            ),
        };

        let mut features = RecognitionFeatures {
            profanity_filter: None,
            enable_word_time_offsets: Some(true),
            enable_word_confidence: Some(true),
            enable_automatic_punctuation: Some(true),
            multi_channel_mode: None,
            diarization_config: None,
            max_alternatives: None,
        };

        if let Some(config) = transcription_config {
            if config.enable_profanity_filter {
                features.profanity_filter = Some(true);
            }

            // Check if multi-channel mode is enabled and model is not latest_short
            if audio_config.channels.as_ref().is_some_and(|c| *c > 1)
                && config.enable_multi_channel
                && !config
                    .model
                    .as_ref()
                    .is_some_and(|m| m.eq_ignore_ascii_case("latest_short"))
            {
                features.multi_channel_mode = Some("SEPARATE_RECOGNITION_PER_CHANNEL".to_string());
            }

            if let Some(ref diarization_config) = config.diarization {
                let min_speakers = diarization_config.min_speaker_count.unwrap_or(2);
                let max_speakers = diarization_config.max_speaker_count.unwrap_or(6);
                features.diarization_config = Some(SpeakerDiarizationConfig {
                    min_speaker_count: min_speakers,
                    max_speaker_count: max_speakers,
                });
            }
        }

        features.max_alternatives = Some(1); //get the best alternative only

        let adaptation = if let Some(config) = transcription_config {
            if !config.phrases.is_empty() {
                let phrase_items: Vec<PhraseItem> = config
                    .phrases
                    .iter()
                    .map(|phrase| PhraseItem {
                        value: phrase.value.clone(),
                        boost: phrase.boost,
                    })
                    .collect();

                Some(SpeechAdaptation {
                    phrase_sets: vec![AdaptationPhraseSet {
                        inline_phrase_set: PhraseSet {
                            phrases: phrase_items,
                        },
                    }],
                })
            } else {
                None
            }
        } else {
            None
        };

        let language_codes = transcription_config.and_then(|c| c.language_codes.clone());

        let model = transcription_config.and_then(|c| c.model.clone());

        let config = RecognitionConfig {
            auto_decoding_config,
            explicit_decoding_config,
            model,
            language_codes,
            features,
            adaptation,
        };

        let files: Vec<BatchRecognizeFileMetadata> = audio_gcs_uris
            .into_iter()
            .map(|uri| BatchRecognizeFileMetadata { uri })
            .collect();

        // Always use inline response config
        let recognition_output_config = RecognitionOutputConfig {
            inline_response_config: InlineOutputConfig {},
        };

        let recognizer_path = format!(
            "projects/{}/locations/{}/recognizers/_",
            self.auth.project_id(),
            self.location
        );

        let request_body = StartBatchRecognizeRequest {
            config,
            config_mask: None,
            files,
            recognition_output_config,
            processing_strategy: None, // Use default processing, which is as soon as possible
        };

        let uri = format!("{}/{}:batchRecognize", BASE_URL, recognizer_path);

        let body = serde_json::to_vec(&request_body).map_err(|e| {
            (
                request_id.to_string(),
                golem_stt::http::Error::Generic(format!("Failed to serialize request: {}", e)),
            )
        })?;

        self.make_authenticated_request(&uri, &request_id, Method::POST, Some(body))
            .await
    }

    async fn get_batch_recognize(
        &mut self,
        request_id: &str,
        operation_name: &str,
    ) -> Result<GetBatchRecognizeOperationResponse, SttError> {
        let uri = format!("{}/{}", BASE_URL, operation_name);

        self.make_authenticated_request(&uri, &request_id, Method::GET, None)
            .await
    }

    async fn wait_for_batch_recognize_completion(
        &mut self,
        request_id: &str,
        operation_name: &str,
        max_wait_time: Duration,
    ) -> Result<GetBatchRecognizeOperationResponse, SttError> {
        let start_time = std::time::Instant::now();
        let poll_interval = Duration::from_secs(10);

        while start_time.elapsed() < max_wait_time {
            let response = self.get_batch_recognize(request_id, operation_name).await?;

            if response.done {
                if response.error.is_some() {
                    return Err(SttError::APIInternalServerError {
                        request_id: request_id.to_string(),
                        provider_error: format!("Operation failed: {:?}", response.error),
                    });
                }
                return Ok(response);
            }

            self.runtime.sleep(poll_interval).await;
        }

        Err(SttError::APIInternalServerError {
            request_id: operation_name.to_string(),
            provider_error: format!(
                "Operation did not complete within {} seconds",
                max_wait_time.as_secs()
            ),
        })
    }

    async fn delete_batch_recognize(
        &mut self,
        request_id: &str,
        operation_name: &str,
    ) -> Result<(), SttError> {
        let uri = format!("{}/{}", BASE_URL, operation_name);

        let _: serde_json::Value = self
            .make_authenticated_request(&uri, request_id, Method::DELETE, None)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Ref, RefCell},
        collections::VecDeque,
        time::Duration,
    };

    use http::{Response, StatusCode};

    use super::*;
    use crate::transcription::gcp_auth::{GcpAuth, ServiceAccountKey};

    struct MockHttpClient {
        pub responses: RefCell<VecDeque<Result<Response<Vec<u8>>, golem_stt::http::Error>>>,
        pub captured_requests: RefCell<Vec<Request<Vec<u8>>>>,
    }

    impl MockHttpClient {
        pub fn new() -> Self {
            Self {
                responses: RefCell::new(VecDeque::new()),
                captured_requests: RefCell::new(Vec::new()),
            }
        }

        pub fn expect_response(&self, response: Response<Vec<u8>>) {
            self.responses.borrow_mut().push_back(Ok(response));
        }

        pub fn get_captured_requests(&self) -> Ref<Vec<Request<Vec<u8>>>> {
            self.captured_requests.borrow()
        }

        pub fn clear_captured_requests(&self) {
            self.captured_requests.borrow_mut().clear();
        }

        pub fn captured_request_count(&self) -> usize {
            self.captured_requests.borrow().len()
        }

        pub fn last_captured_request(&self) -> Option<Ref<Request<Vec<u8>>>> {
            let borrow = self.captured_requests.borrow();
            if borrow.is_empty() {
                None
            } else {
                Some(Ref::map(borrow, |requests| requests.last().unwrap()))
            }
        }
    }

    impl HttpClient for MockHttpClient {
        async fn execute(
            &self,
            request: Request<Vec<u8>>,
        ) -> Result<Response<Vec<u8>>, golem_stt::http::Error> {
            self.captured_requests.borrow_mut().push(request);
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(golem_stt::http::Error::Generic(
                    "unexpected error".to_string(),
                )))
        }
    }

    struct MockRuntime {
        sleep_calls: RefCell<Vec<Duration>>,
    }

    impl MockRuntime {
        fn new() -> Self {
            Self {
                sleep_calls: RefCell::new(Vec::new()),
            }
        }

        fn get_sleep_calls(&self) -> Ref<Vec<Duration>> {
            self.sleep_calls.borrow()
        }
    }

    impl golem_stt::runtime::AsyncRuntime for MockRuntime {
        async fn sleep(&self, duration: Duration) {
            self.sleep_calls.borrow_mut().push(duration);
        }
    }

    fn create_test_service_account_key() -> ServiceAccountKey {
        ServiceAccountKey {
            key_type: "service_account".to_string(),
            project_id: "test-project-id".to_string(),
            private_key_id: "test-key-id".to_string(),
            private_key: "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC3nmCgsAlob5Fb\n8J81FCw+80nAilI2soaayyr7nYUPQJORtu4mNEOSdnLBTk4RFvaH8UAJ7h21fcF2\nUEn3YOB0yUYIKBDS3uB60oplwJOnbis3lAlsT0VZ/UtngF6zNhJBVpz/RrwSJ1Po\nTnOrlkrrRXgPK6t5AxuR0n+h4P3YMU7hLZ46A5m/7YLJdWkVE1p3GYcrlltm2sos\nWWUpiNGIDflG42tlJVwG+QXL7J9D4ua/jbkFOvKI0Dl893ka0gkUCR0T0Cm1TRwo\nbBTBV/b/YXVCSJug0KsIIxYG0izSzlETH0Ql9tl6G+q0C4H0HUkN/UZ3QFYPmZUs\nX3Wu8DmvAgMBAAECggEBAKIU4YK2IXfYk90uZ7q41d2zb7TP5IZ3zC2zjXuRrjSq\nchi7+zgqBkOw3tcXwf1/4ZpaMIcTc5ITMcS4VrJRB5DPYkws4bziFBEW7CepeCzh\nKLDksfSzfKpU1kzEmdNjtXWLeQY1cCouIPj810ntXrCTH8l0aOZnAd0UjKleK3S7\ngva0IYHvCtoYFdvvwCOfxRQKAufcwotkgJPs6m95QJYwwfN3EaZi7duuNu0fKRkH\nu2sfRqDcJR3Yo4Nt9LhqB/OfkfL0TuzkNbXi0ZsUTJ5pFRx1m+Gtbb3qC95MBeey\ng/F9slQwRpDyJdxIrNVn7tv5tsd8v+4USwAC+cklQnECgYEA2wFvJ4KykuKG4RXO\nbWG0pavchTIixcC86y1ht/OxZFx13KmVzyE0PiOGTozAJCAHu1JK5gLxgGzXgLLr\nnT55kBvTzQ7+HQh+jhjrIIruicfiugzEQ6MivSw0pnk2Lkta25AeHuW1bKao1dOr\nnBDrtAZ1oKybBcna8SkYHprXh/0CgYEA1qKwRoZjfokzwmLwCyXDQyDKgUM0OOLq\nMXsCVv8BXltoSH5/vlDKSePs+4Er3o596QJRUosuwLgfIHsqFSFpUDk3lIctkqOt\nT1P1tjBZg8qMCSFzIwqsyj0lXN5IK6Zqvi7WikVVQ7gN3Stu4H0C9OgyV+kzHlNW\niV8cfvMJChsCgYAWnQRMMRudPRSuQyEofDE59g/0FOQwRSF8qxfu9ZO4iC+HVF9q\nnsQVMnfYvoHMeR4zQmEHdQBYwWRTHqZjeyL0NVteThEBEHJ426vTlWTiByirC0xs\nq3iXzeu10Mg+aXt9NllV2WQtTtwaEBwlJj4gPZaBu7DaHSilRBgAeP6ORQKBgGsV\nZe75s3/5AdrUs8BMCdxe6smM9uv+wisHnQY8Wblyz1eDzUXtVs+AqMZeDr4Nx2HO\nJzaQfDXoZpc0+6zpK3q74S/4NVN418nBMNDB1Jc9IZqYlrH/7G9GDHMF72nfsFfM\nVHtN1hlgJYKX3cygci4v/pX/oeJaX81Pp47qwDLLAoGAJadd2du9Nrd5WNohsPBH\nNGtq6QMJsjAABKkFXlqFM4Jsc/zaEOa/fsLCp6lbrVEqvHZGFc+OoukDlhY+c3QU\nSFVTtnsNi4YIbd8xNUpRNw7neShlG64wG0tLTI+y7a7Xh7GWkfYdfA950O8QEh46\nrecURYwOhS+7tjhb0xXs4kU=\n-----END PRIVATE KEY-----".to_string(),
            client_email: "test@test-project-id.iam.gserviceaccount.com".to_string(),
            client_id: "test-client-id".to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            auth_provider_x509_cert_url: "https://www.googleapis.com/oauth2/v1/certs".to_string(),
            client_x509_cert_url: "https://www.googleapis.com/robot/v1/metadata/x509/test%40test-project-id.iam.gserviceaccount.com".to_string(),
        }
    }

    #[wstd::test]
    async fn test_start_batch_recognize_basic_request() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
               Response::builder()
                   .status(StatusCode::OK)
                   .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                   .unwrap(),
           );

        let mock_response = r#"{
               "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
               "metadata": {
                   "createTime": "2023-01-01T00:00:00Z"
               },
               "done": false
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(mock_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        // Test basic audio config with no transcription config
        let audio_config = AudioConfig {
            format: AudioFormat::Wav,
            sample_rate_hertz: Some(16000),
            channels: Some(1),
        };

        let _result = client
            .start_batch_recognize(
                "test-request-id",
                vec!["gs://bucket/audio.wav".to_string()],
                &audio_config,
                None, // No transcription config
            )
            .await
            .unwrap();

        // We should have 2 requests: one for token exchange, one for batch recognize
        assert_eq!(mock_client.captured_request_count(), 2);

        // Verify the batch recognize request was constructed correctly
        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartBatchRecognizeRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartBatchRecognizeRequest {
            config: RecognitionConfig {
                model: None,
                language_codes: None,
                features: RecognitionFeatures {
                    profanity_filter: None,
                    enable_word_time_offsets: Some(true),
                    enable_word_confidence: Some(true),
                    enable_automatic_punctuation: Some(true),
                    multi_channel_mode: None,
                    diarization_config: None,
                    max_alternatives: Some(1),
                },
                adaptation: None,
                auto_decoding_config: Some(AutoDetectDecodingConfig {}),
                explicit_decoding_config: None,
            },
            config_mask: None,
            files: vec![BatchRecognizeFileMetadata {
                uri: "gs://bucket/audio.wav".to_string(),
            }],
            recognition_output_config: RecognitionOutputConfig {
                inline_response_config: InlineOutputConfig {},
            },
            processing_strategy: None,
        };

        assert_eq!(
            actual_request, expected_request,
            "Basic request should match expected structure"
        );

        // Verify the request headers and URL
        assert_eq!(request.method(), "POST");
        assert_eq!(
               request.uri().to_string(),
               "https://speech.googleapis.com/v2/projects/test-project-id/locations/us-central1/recognizers/_:batchRecognize"
           );

        let auth_header = request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth_header, "Bearer test-access-token");
    }

    #[wstd::test]
    async fn test_start_batch_recognize_with_multi_channel() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
               Response::builder()
                   .status(StatusCode::OK)
                   .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                   .unwrap(),
           );

        let mock_response = r#"{
               "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
               "metadata": {},
               "done": false
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(mock_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let audio_config = AudioConfig {
            format: AudioFormat::Wav,
            sample_rate_hertz: Some(16000),
            channels: Some(2), // Multi-channel audio
        };

        let transcription_config = TranscriptionConfig {
            language_codes: Some(vec!["en-US".to_string()]),
            model: Some("latest_long".to_string()), // Not latest_short, so multi-channel should work
            enable_profanity_filter: false,
            diarization: None,
            enable_multi_channel: true, // Enable multi-channel
            phrases: vec![],
        };

        let _result = client
            .start_batch_recognize(
                "test-request-id",
                vec!["gs://bucket/audio1.wav".to_string()],
                &audio_config,
                Some(&transcription_config),
            )
            .await
            .unwrap();

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartBatchRecognizeRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartBatchRecognizeRequest {
            config: RecognitionConfig {
                model: Some("latest_long".to_string()),
                language_codes: Some(vec!["en-US".to_string()]),
                features: RecognitionFeatures {
                    profanity_filter: None,
                    enable_word_time_offsets: Some(true),
                    enable_word_confidence: Some(true),
                    enable_automatic_punctuation: Some(true),
                    multi_channel_mode: Some("SEPARATE_RECOGNITION_PER_CHANNEL".to_string()),
                    diarization_config: None,
                    max_alternatives: Some(1),
                },
                adaptation: None,
                auto_decoding_config: Some(AutoDetectDecodingConfig {}),
                explicit_decoding_config: None,
            },
            config_mask: None,
            files: vec![BatchRecognizeFileMetadata {
                uri: "gs://bucket/audio1.wav".to_string(),
            }],
            recognition_output_config: RecognitionOutputConfig {
                inline_response_config: InlineOutputConfig {},
            },
            processing_strategy: None,
        };

        assert_eq!(
            actual_request, expected_request,
            "Multi-channel request should match expected structure"
        );
    }

    #[wstd::test]
    async fn test_start_batch_recognize_with_speaker_diarization() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                    .unwrap(),
            );

        let mock_response = r#"{
                "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
                "metadata": {},
                "done": false
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(mock_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let audio_config = AudioConfig {
            format: AudioFormat::Flac,
            sample_rate_hertz: Some(16000),
            channels: Some(1),
        };

        let transcription_config = TranscriptionConfig {
            language_codes: Some(vec!["en-US".to_string()]),
            model: Some("latest_long".to_string()),
            enable_profanity_filter: false,
            diarization: Some(DiarizationConfig {
                enabled: true,
                min_speaker_count: Some(3),
                max_speaker_count: Some(5),
            }),
            // Custom max speakers
            enable_multi_channel: false,
            phrases: vec![],
        };

        let _result = client
            .start_batch_recognize(
                "test-request-id",
                vec!["gs://bucket/audio1.flac".to_string()],
                &audio_config,
                Some(&transcription_config),
            )
            .await
            .unwrap();

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartBatchRecognizeRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartBatchRecognizeRequest {
            config: RecognitionConfig {
                model: Some("latest_long".to_string()),
                language_codes: Some(vec!["en-US".to_string()]),
                features: RecognitionFeatures {
                    profanity_filter: None,
                    enable_word_time_offsets: Some(true),
                    enable_word_confidence: Some(true),
                    enable_automatic_punctuation: Some(true),
                    multi_channel_mode: None,
                    diarization_config: Some(SpeakerDiarizationConfig {
                        min_speaker_count: 3,
                        max_speaker_count: 5,
                    }),
                    max_alternatives: Some(1),
                },
                adaptation: None,
                auto_decoding_config: Some(AutoDetectDecodingConfig {}),
                explicit_decoding_config: None,
            },
            config_mask: None,
            files: vec![BatchRecognizeFileMetadata {
                uri: "gs://bucket/audio1.flac".to_string(),
            }],
            recognition_output_config: RecognitionOutputConfig {
                inline_response_config: InlineOutputConfig {},
            },
            processing_strategy: None,
        };

        assert_eq!(
            actual_request, expected_request,
            "Speaker diarization request should match expected structure"
        );
    }

    #[wstd::test]
    async fn test_start_batch_recognize_with_explicit_language() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                    .unwrap(),
            );

        let mock_response = r#"{
                "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
                "metadata": {},
                "done": false
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(mock_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let audio_config = AudioConfig {
            format: AudioFormat::LinearPcm,
            sample_rate_hertz: Some(16000),
            channels: Some(1),
        };

        let transcription_config = TranscriptionConfig {
            language_codes: Some(vec!["es-ES".to_string(), "en-US".to_string()]), // Multiple languages
            model: Some("latest_long".to_string()),
            enable_profanity_filter: false,
            diarization: None,
            enable_multi_channel: false,
            phrases: vec![],
        };

        let _result = client
            .start_batch_recognize(
                "test-request-id",
                vec!["gs://bucket/audio.raw".to_string()],
                &audio_config,
                Some(&transcription_config),
            )
            .await
            .unwrap();

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartBatchRecognizeRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartBatchRecognizeRequest {
            config: RecognitionConfig {
                model: Some("latest_long".to_string()),
                language_codes: Some(vec!["es-ES".to_string(), "en-US".to_string()]),
                features: RecognitionFeatures {
                    profanity_filter: None,
                    enable_word_time_offsets: Some(true),
                    enable_word_confidence: Some(true),
                    enable_automatic_punctuation: Some(true),
                    multi_channel_mode: None,
                    diarization_config: None,
                    max_alternatives: Some(1),
                },
                adaptation: None,
                auto_decoding_config: None,
                explicit_decoding_config: Some(ExplicitDecodingConfig {
                    encoding: "LINEAR16".to_string(),
                    sample_rate_hertz: Some(16000),
                    audio_channel_count: Some(1),
                }),
            },
            config_mask: None,
            files: vec![BatchRecognizeFileMetadata {
                uri: "gs://bucket/audio.raw".to_string(),
            }],
            recognition_output_config: RecognitionOutputConfig {
                inline_response_config: InlineOutputConfig {},
            },
            processing_strategy: None,
        };

        assert_eq!(
            actual_request, expected_request,
            "Explicit language request should match expected structure"
        );
    }

    #[wstd::test]
    async fn test_start_batch_recognize_with_model() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
               Response::builder()
                   .status(StatusCode::OK)
                   .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                   .unwrap(),
           );

        let mock_response = r#"{
               "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
               "metadata": {},
               "done": false
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(mock_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let audio_config = AudioConfig {
            format: AudioFormat::Mp3,
            sample_rate_hertz: None,
            channels: None,
        };

        let transcription_config = TranscriptionConfig {
            language_codes: Some(vec!["en-US".to_string()]),
            model: Some("medical_conversation".to_string()), // User-provided model
            enable_profanity_filter: false,
            diarization: None,
            enable_multi_channel: false,
            phrases: vec![],
        };

        let _result = client
            .start_batch_recognize(
                "test-request-id",
                vec!["gs://bucket/medical_call.mp3".to_string()],
                &audio_config,
                Some(&transcription_config),
            )
            .await
            .unwrap();

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartBatchRecognizeRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartBatchRecognizeRequest {
            config: RecognitionConfig {
                model: Some("medical_conversation".to_string()),
                language_codes: Some(vec!["en-US".to_string()]),
                features: RecognitionFeatures {
                    profanity_filter: None,
                    enable_word_time_offsets: Some(true),
                    enable_word_confidence: Some(true),
                    enable_automatic_punctuation: Some(true),
                    multi_channel_mode: None,
                    diarization_config: None,
                    max_alternatives: Some(1),
                },
                adaptation: None,
                auto_decoding_config: Some(AutoDetectDecodingConfig {}),
                explicit_decoding_config: None,
            },
            config_mask: None,
            files: vec![BatchRecognizeFileMetadata {
                uri: "gs://bucket/medical_call.mp3".to_string(),
            }],
            recognition_output_config: RecognitionOutputConfig {
                inline_response_config: InlineOutputConfig {},
            },
            processing_strategy: None,
        };

        assert_eq!(
            actual_request, expected_request,
            "Model request should match expected structure"
        );
    }

    #[wstd::test]
    async fn test_start_batch_recognize_with_phrases() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
               Response::builder()
                   .status(StatusCode::OK)
                   .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                   .unwrap(),
           );

        let mock_response = r#"{
               "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
               "metadata": {},
               "done": false
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(mock_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let audio_config = AudioConfig {
            format: AudioFormat::WebmOpus,
            sample_rate_hertz: Some(16000),
            channels: Some(1),
        };

        let transcription_config = TranscriptionConfig {
            language_codes: Some(vec!["en-US".to_string()]),
            model: Some("latest_short".to_string()),
            enable_profanity_filter: false,
            diarization: None,
            enable_multi_channel: false,
            phrases: vec![
                Phrase {
                    value: "Google Cloud Platform".to_string(),
                    boost: Some(10.0), // Phrase with boost
                },
                Phrase {
                    value: "machine learning".to_string(),
                    boost: None, // Phrase without boost
                },
                Phrase {
                    value: "artificial intelligence".to_string(),
                    boost: Some(15.5), // Another phrase with boost
                },
            ],
        };

        let _result = client
            .start_batch_recognize(
                "test-request-id",
                vec!["gs://bucket/tech_talk.webm".to_string()],
                &audio_config,
                Some(&transcription_config),
            )
            .await
            .unwrap();

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartBatchRecognizeRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartBatchRecognizeRequest {
            config: RecognitionConfig {
                model: Some("latest_short".to_string()),
                language_codes: Some(vec!["en-US".to_string()]),
                features: RecognitionFeatures {
                    profanity_filter: None,
                    enable_word_time_offsets: Some(true),
                    enable_word_confidence: Some(true),
                    enable_automatic_punctuation: Some(true),
                    multi_channel_mode: None,
                    diarization_config: None,
                    max_alternatives: Some(1),
                },
                adaptation: Some(SpeechAdaptation {
                    phrase_sets: vec![AdaptationPhraseSet {
                        inline_phrase_set: PhraseSet {
                            phrases: vec![
                                PhraseItem {
                                    value: "Google Cloud Platform".to_string(),
                                    boost: Some(10.0),
                                },
                                PhraseItem {
                                    value: "machine learning".to_string(),
                                    boost: None,
                                },
                                PhraseItem {
                                    value: "artificial intelligence".to_string(),
                                    boost: Some(15.5),
                                },
                            ],
                        },
                    }],
                }),
                auto_decoding_config: Some(AutoDetectDecodingConfig {}),
                explicit_decoding_config: None,
            },
            config_mask: None,
            files: vec![BatchRecognizeFileMetadata {
                uri: "gs://bucket/tech_talk.webm".to_string(),
            }],
            recognition_output_config: RecognitionOutputConfig {
                inline_response_config: InlineOutputConfig {},
            },
            processing_strategy: None,
        };

        assert_eq!(
            actual_request, expected_request,
            "Phrases request should match expected structure"
        );
    }

    #[wstd::test]
    async fn test_start_batch_recognize_with_profanity_filter() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
               Response::builder()
                   .status(StatusCode::OK)
                   .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                   .unwrap(),
           );

        let mock_response = r#"{
               "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
               "metadata": {},
               "done": false
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(mock_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let audio_config = AudioConfig {
            format: AudioFormat::Mp4,
            sample_rate_hertz: None,
            channels: None,
        };

        let transcription_config = TranscriptionConfig {
            language_codes: Some(vec!["en-US".to_string()]),
            model: Some("latest_long".to_string()),
            enable_profanity_filter: true, // Enable profanity filter
            diarization: None,
            enable_multi_channel: false,
            phrases: vec![],
        };

        let _result = client
            .start_batch_recognize(
                "test-request-id",
                vec!["gs://bucket/audio.mp4".to_string()],
                &audio_config,
                Some(&transcription_config),
            )
            .await
            .unwrap();

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartBatchRecognizeRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartBatchRecognizeRequest {
            config: RecognitionConfig {
                model: Some("latest_long".to_string()),
                language_codes: Some(vec!["en-US".to_string()]),
                features: RecognitionFeatures {
                    profanity_filter: Some(true),
                    enable_word_time_offsets: Some(true),
                    enable_word_confidence: Some(true),
                    enable_automatic_punctuation: Some(true),
                    multi_channel_mode: None,
                    diarization_config: None,
                    max_alternatives: Some(1),
                },
                adaptation: None,
                auto_decoding_config: Some(AutoDetectDecodingConfig {}),
                explicit_decoding_config: None,
            },
            config_mask: None,
            files: vec![BatchRecognizeFileMetadata {
                uri: "gs://bucket/audio.mp4".to_string(),
            }],
            recognition_output_config: RecognitionOutputConfig {
                inline_response_config: InlineOutputConfig {},
            },
            processing_strategy: None,
        };

        assert_eq!(
            actual_request, expected_request,
            "Profanity filter request should match expected structure"
        );
    }

    #[wstd::test]
    async fn test_delete_batch_recognize() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                    .unwrap(),
            );

        mock_client.expect_response(
            Response::builder()
                .status(StatusCode::OK)
                .body(b"{}".to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let operation_name =
            "projects/test-project-id/locations/us-central1/operations/operation-123";
        let result = client
            .delete_batch_recognize("test-request-id", operation_name)
            .await;

        assert!(result.is_ok());

        // We should have 2 requests: one for token exchange, one for delete
        assert_eq!(mock_client.captured_request_count(), 2);

        let request = mock_client.last_captured_request().unwrap();
        assert_eq!(request.method(), "DELETE");
        assert_eq!(
            request.uri().to_string(),
            format!("https://speech.googleapis.com/v2/{}", operation_name)
        );

        let auth_header = request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth_header, "Bearer test-access-token");
    }

    #[wstd::test]
    async fn test_wait_for_batch_recognize_completion() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response (called multiple times for each polling request)
        mock_client.expect_response(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                    .unwrap(),
            );

        // First poll - operation is not done
        let in_progress_response = r#"{
                "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
                "metadata": {
                    "createTime": "2023-01-01T00:00:00Z",
                    "progressPercent": 25
                },
                "done": false
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(in_progress_response.as_bytes().to_vec())
                .unwrap(),
        );

        // Second poll - operation is completed
        let completed_response = r#"{
                    "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
                    "metadata": {
                        "createTime": "2023-01-01T00:00:00Z",
                        "updateTime": "2023-01-01T00:05:00Z",
                        "progressPercent": 100
                    },
                    "done": true,
                    "response": {
                        "results": {
                            "gs://bucket/audio.wav": {
                                "inlineResult": {
                                    "transcript": {
                                        "results": [
                                            {
                                                "alternatives": [
                                                    {
                                                        "transcript": "Hello world",
                                                        "confidence": 0.95
                                                    }
                                                ]
                                            }
                                        ]
                                    }
                                }
                            }
                        },
                        "totalBilledDuration": "30s"
                    }
                }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(completed_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let operation_name =
            "projects/test-project-id/locations/us-central1/operations/operation-123";
        let response = client
            .wait_for_batch_recognize_completion(
                "test-request-id",
                operation_name,
                Duration::from_secs(3600),
            )
            .await
            .unwrap();

        assert_eq!(response.done, true);
        assert!(response.response.is_some());
        assert!(response.error.is_none());

        // Should have made exactly 3 requests: 1 OAuth + 2 polling requests
        assert_eq!(mock_client.captured_request_count(), 3);

        // Should have called sleep at least once
        let sleep_calls = client.runtime.get_sleep_calls();
        assert!(!sleep_calls.is_empty());
        assert_eq!(
            sleep_calls[0],
            Duration::from_secs(10),
            "First sleep should be 10 seconds"
        );

        // Verify the polling requests were get_batch_recognize calls
        let captured_requests = mock_client.get_captured_requests();

        // Check the first polling request (index 1, since index 0 is OAuth)
        let first_poll_request = &captured_requests[1];
        assert_eq!(first_poll_request.method(), "GET");
        assert_eq!(
            first_poll_request.uri().to_string(),
            format!("https://speech.googleapis.com/v2/{}", operation_name)
        );

        let auth_header = first_poll_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth_header, "Bearer test-access-token");

        let second_poll_request = &captured_requests[2];
        assert_eq!(second_poll_request.method(), "GET");
        assert_eq!(
            second_poll_request.uri().to_string(),
            format!("https://speech.googleapis.com/v2/{}", operation_name)
        );
    }

    #[wstd::test]
    async fn test_wait_for_batch_recognize_completion_failure() {
        let mock_client = Rc::new(MockHttpClient::new());

        // Mock the OAuth token exchange response
        mock_client.expect_response(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                    .unwrap(),
            );

        // Mock operation response with error
        let failed_response = r#"{
                "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
                "metadata": {
                    "createTime": "2023-01-01T00:00:00Z",
                    "updateTime": "2023-01-01T00:02:00Z",
                    "progressPercent": 100
                },
                "done": true,
                "error": {
                    "code": 3,
                    "message": "Audio file format is not supported",
                    "details": []
                }
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(failed_response.as_bytes().to_vec())
                .unwrap(),
        );

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let operation_name =
            "projects/test-project-id/locations/us-central1/operations/operation-123";
        let result = client
            .wait_for_batch_recognize_completion(
                "test-request-id",
                operation_name,
                Duration::from_secs(3600),
            )
            .await;

        assert!(result.is_err());

        match result.unwrap_err() {
            SttError::APIInternalServerError {
                request_id,
                provider_error,
            } => {
                assert_eq!(request_id, "test-request-id");
                assert!(provider_error.contains("Operation failed"));
                assert!(provider_error.contains("Audio file format is not supported"));
            }
            other => panic!("Expected APIInternalServerError, got: {:?}", other),
        }

        // Should have made exactly 2 requests: 1 OAuth + 1 polling request
        assert_eq!(mock_client.captured_request_count(), 2);

        // Verify the polling request
        let captured_requests = mock_client.get_captured_requests();
        let poll_request = &captured_requests[1];
        assert_eq!(poll_request.method(), "GET");
        assert_eq!(
            poll_request.uri().to_string(),
            format!("https://speech.googleapis.com/v2/{}", operation_name)
        );

        let auth_header = poll_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth_header, "Bearer test-access-token");
    }

    #[wstd::test]
    async fn test_wait_for_batch_recognize_completion_timeout() {
        let mock_client = Rc::new(MockHttpClient::new());

        mock_client.expect_response(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(br#"{"access_token":"test-access-token","token_type":"Bearer","expires_in":3600}"#.to_vec())
                    .unwrap(),
            );

        for _ in 0..100 {
            // Always return IN_PROGRESS to simulate timeout
            let in_progress_response = r#"{
                    "name": "projects/test-project-id/locations/us-central1/operations/operation-123",
                    "metadata": {
                        "createTime": "2023-01-01T00:00:00Z",
                        "progressPercent": 50
                    },
                    "done": false
                }"#;

            mock_client.expect_response(
                Response::builder()
                    .status(200)
                    .body(in_progress_response.as_bytes().to_vec())
                    .unwrap(),
            );
        }

        struct MockRuntime {
            elapsed_time: std::cell::RefCell<Duration>,
        }

        impl MockRuntime {
            fn new() -> Self {
                Self {
                    elapsed_time: std::cell::RefCell::new(Duration::from_secs(0)),
                }
            }
        }

        impl golem_stt::runtime::AsyncRuntime for MockRuntime {
            async fn sleep(&self, duration: Duration) {
                // Simulate time passing
                let mut elapsed = self.elapsed_time.borrow_mut();
                *elapsed += duration;
            }
        }

        let mock_runtime = MockRuntime::new();

        let service_account_key = create_test_service_account_key();
        let auth = GcpAuth::new(service_account_key, mock_client.clone()).unwrap();

        let mut client = SpeechToTextClient::new(
            auth,
            mock_client.clone(),
            Some("us-central1".to_string()),
            mock_runtime,
        );

        let operation_name =
            "projects/test-project-id/locations/us-central1/operations/operation-123";
        let result = client
            .wait_for_batch_recognize_completion(
                "test-request-id",
                operation_name,
                Duration::from_millis(5), // Very short timeout
            )
            .await;

        assert!(
            client.runtime.elapsed_time.borrow().as_millis() > 0,
            "Elapsed time should be greater than zero"
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            SttError::APIInternalServerError {
                request_id,
                provider_error,
            } => {
                assert_eq!(request_id, operation_name);
                assert!(provider_error.contains("Operation did not complete within"));
            }
            _ => panic!("Expected APIInternalServerError timeout error"),
        }
    }
}
