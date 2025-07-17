use std::{rc::Rc, time::Duration};

use crate::aws::{S3Client, S3Service, TranscribeClient, TranscribeOutput, TranscribeService};
use golem_stt::{
    client::{ReqwestHttpClient, SttProviderClient},
    error::Error,
    runtime::AsyncRuntime,
    runtime::WasiAyncRuntime,
};

use bytes::Bytes;
use log::trace;
use wasi_async_runtime::Reactor;

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
    pub channels: Option<u8>,
}

fn validate_request_id(request_id: &str) -> Result<(), String> {
    if request_id.is_empty() {
        return Err("Request ID cannot be empty".to_string());
    }

    // Check length - Transcribe support up to 200 characters for our use case
    if request_id.len() > 200 {
        return Err(
            "Request ID too long (max 200 characters for S3 and Transcribe compatibility)"
                .to_string(),
        );
    }

    // https://docs.aws.amazon.com/transcribe/latest/APIReference/API_CreateVocabulary.html#transcribe-CreateVocabulary-request-VocabularyName
    // AWS Transcribe vocabulary name pattern: ^[0-9a-zA-Z._-]+$ which is also S3-safe
    let is_valid_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';

    if !request_id.chars().all(is_valid_char) {
        return Err(
                "Request ID contains invalid characters. Only alphanumeric characters, hyphens (-), underscores (_), and dots (.) are allowed for S3 and Transcribe compatibility".to_string()
            );
    }

    if request_id.to_lowercase().starts_with("aws-") {
        return Err(
            "Request ID cannot start with 'aws-' (reserved prefix for AWS services)".to_string(),
        );
    }

    // Ensure it starts with an alphanumeric character (good practice for both S3 and Transcribe)
    if !request_id.chars().next().unwrap().is_ascii_alphanumeric() {
        return Err("Request ID must start with an alphanumeric character".to_string());
    }

    if request_id.ends_with('-') || request_id.ends_with('_') || request_id.ends_with('.') {
        return Err("Request ID cannot end with hyphens, underscores, or dots".to_string());
    }

    let problematic_patterns = ["--", "__", "..", "-_", "_-", "-.", "._", "_.", ".-"];
    for pattern in &problematic_patterns {
        if request_id.contains(pattern) {
            return Err("Request ID cannot contain consecutive special characters".to_string());
        }
    }

    if request_id.starts_with('.') {
        return Err(
            "Request ID cannot start with a dot (reserved for file extensions)".to_string(),
        );
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub language: Option<String>,
    pub model: Option<String>,
    pub enable_speaker_diarization: bool,
    pub vocabulary: Vec<String>,
}

pub struct TranscribeApi<S3: S3Service, TC: TranscribeService, RT: AsyncRuntime> {
    bucket_name: String,
    s3_client: S3,
    transcribe_client: TC,
    runtime: RT,
}

#[allow(unused)]
impl<S3: S3Service, TC: TranscribeService, RT: AsyncRuntime> TranscribeApi<S3, TC, RT> {
    pub fn new(bucket_name: String, s3_client: S3, transcribe_client: TC, runtime: RT) -> Self {
        Self {
            bucket_name,
            s3_client,
            transcribe_client,
            runtime,
        }
    }

    // pub fn get_supported_languages(&self) -> &[Language] {
    //     &AWS_TRANSCRIBE_SUPPORTED_LANGUAGES
    // }
}

impl
    TranscribeApi<S3Client<ReqwestHttpClient>, TranscribeClient<ReqwestHttpClient>, WasiAyncRuntime>
{
    pub fn live(
        bucket_name: String,
        access_key: String,
        secret_key: String,
        region: String,
        reactor: Reactor,
    ) -> Self {
        let reqwest_http_client = ReqwestHttpClient::new(reactor.clone());

        let s3_client = S3Client::new(
            access_key.clone(),
            secret_key.clone(),
            region.clone(),
            reqwest_http_client.clone(),
        );

        let transcribe_client = TranscribeClient::new(
            access_key.clone(),
            secret_key.clone(),
            region.clone(),
            reqwest_http_client.clone(),
        );

        Self::new(
            bucket_name,
            s3_client,
            transcribe_client,
            WasiAyncRuntime::new(reactor.clone()),
        )
    }
}

impl<S3: S3Service, TC: TranscribeService, RT: AsyncRuntime>
    SttProviderClient<TranscriptionRequest, TranscriptionResponse, Error>
    for TranscribeApi<S3, TC, RT>
{
    async fn transcribe_audio(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionResponse, Error> {
        trace!("Sending request to AWS Transcribe API: {request:?}");

        let request_id: Rc<str> = Rc::from(request.request_id);
        let audio_size_bytes = request.audio.len();
        let req_language = request
            .transcription_config
            .as_ref()
            .and_then(|config| config.language.clone());

        validate_request_id(&request_id).map_err(|validation_error| Error::APIBadRequest {
            request_id: request_id.to_string(),
            provider_error: format!("Invalid request ID: {}", validation_error),
        })?;

        if let Some(ref config) = request.transcription_config {
            if !config.vocabulary.is_empty() && config.language.is_none() {
                return Err(Error::APIBadRequest {
                            request_id: request_id.to_string(),
                            provider_error: "Vocabulary can only be used when a specific language is provided. Cannot be used with automatic language detection.".to_string(),
                        });
            }

            if config.model.is_some() && config.language.is_none() {
                return Err(Error::APIBadRequest {
                            request_id: request_id.to_string(),
                            provider_error: "Model settings can only be used when a specific language is provided. Cannot be used with automatic language detection.".to_string(),
                        });
            }
        }

        let object_key = format!("{}/audio.{}", request_id, request.audio_config.format);

        self.s3_client
            .put_object(&request_id, &self.bucket_name, &object_key, request.audio)
            .await?;

        let vocabulary_name = if let Some(ref config) = request.transcription_config {
            if !config.vocabulary.is_empty() {
                let language_code = config.language.as_ref().unwrap(); // Safe due to validation above

                let res = self
                    .transcribe_client
                    .create_vocabulary(
                        request_id.to_string(),
                        language_code.clone(),
                        config.vocabulary.clone(),
                    )
                    .await?;

                if res.vocabulary_state == "FAILED" {
                    return Err(Error::APIBadRequest {
                        request_id: request_id.to_string(),
                        provider_error: format!(
                            "Vocabulary creation failed: {}",
                            res.failure_reason
                                .unwrap_or_else(|| "Unknown error".to_string())
                        ),
                    });
                }

                if res.vocabulary_state == "PENDING" {
                    self.transcribe_client
                        .wait_for_vocabulary_ready(
                            &self.runtime,
                            &request_id,
                            Duration::from_secs(300),
                        )
                        .await?;
                }

                Some(request_id.to_string()) // vocabulary name is the request_id
            } else {
                None
            }
        } else {
            None
        };

        let res = self
            .transcribe_client
            .start_transcription_job(
                request_id.to_string(),
                format!("s3://{}/{object_key}", &self.bucket_name),
                request.audio_config,
                request.transcription_config,
                vocabulary_name,
            )
            .await?;

        if res.transcription_job.transcription_job_status == "FAILED" {
            return Err(Error::APIBadRequest {
                request_id: request_id.to_string(),
                provider_error: format!(
                    "Transcription job creation failed: {}",
                    res.transcription_job
                        .failure_reason
                        .unwrap_or_else(|| "Unknown error".to_string())
                ),
            });
        }

        let completed_transcription_job =
            if res.transcription_job.transcription_job_status == "IN_PROGRESS" {
                self.transcribe_client
                    .wait_for_transcription_job_completion(
                        &self.runtime,
                        &request_id,
                        Duration::from_secs(3600 * 6),
                    )
                    .await?
                    .transcription_job
            } else {
                res.transcription_job
            };

        if let Some(transcript) = completed_transcription_job.transcript {
            if let Some(transcript_uri) = transcript.transcript_file_uri {
                let transcribe_output = self
                    .transcribe_client
                    .download_transcript_json(request_id.as_ref(), &transcript_uri)
                    .await?;

                self.transcribe_client
                    .delete_vocabulary(&request_id)
                    .await?;

                self.s3_client
                    .delete_object(&request_id, &self.bucket_name, &object_key)
                    .await?;

                Ok(TranscriptionResponse {
                    audio_size_bytes,
                    language: req_language.unwrap_or_default(),
                    aws_transcription: transcribe_output,
                })
            } else {
                Err(golem_stt::error::Error::APIUnknown {
                    request_id: request_id.to_string(),
                    provider_error: "Transcription completed but no transcript file URI found"
                        .to_string(),
                })
            }
        } else {
            Err(golem_stt::error::Error::APIUnknown {
                request_id: request_id.to_string(),
                provider_error: "Transcription completed but no transcript found".to_string(),
            })
        }
    }
}

pub struct TranscriptionRequest {
    pub request_id: String,
    pub audio: Bytes,
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
    pub language: String,
    pub aws_transcription: TranscribeOutput,
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Ref, RefCell},
        collections::VecDeque,
    };

    use golem_stt::client;
    use wasi_async_runtime::block_on;

    use crate::aws::{
        CreateVocabularyResponse, GetTranscriptionJobResponse, GetVocabularyResponse, S3Service,
        StartTranscriptionJobResponse, TranscribeResults, Transcript, TranscriptText,
        TranscriptionJob,
    };

    use super::*;

    #[test]
    fn test_validate_request_id_valid_cases() {
        // Valid request IDs for both S3 and Transcribe
        let valid_ids = vec![
            "abc123",
            "request-123",
            "my_request_456",
            "test-request_789",
            "a1b2c3",
            "RequestID123",
            "user123-session456",
            "batch_job_001",
            // Valid cases with dots (Transcribe allows these)
            "request.123",
            "user.session.456",
            "v1.2.3",
            "api.request.789",
            "test-1.0_beta",
            "service.endpoint.call",
            "user123.session-456_temp",
            "namespace.resource.id",
            // Edge cases
            "a",                // single character
            "1",                // single digit
            "A1",               // mixed case
            "test123_final.v1", // complex but valid
        ];

        for id in valid_ids {
            assert!(
                validate_request_id(id).is_ok(),
                "Expected '{}' to be valid for both S3 and Transcribe, but validation failed: {:?}",
                id,
                validate_request_id(id)
            );
        }
    }

    #[test]
    fn test_validate_request_id_invalid_cases() {
        let long_id = "a".repeat(201);

        let test_cases = vec![
            // Empty string
            ("", "Request ID cannot be empty"),
            // Too long
            (&long_id, "Request ID too long"),
            // Invalid characters (not in ^[0-9a-zA-Z._-]+$ pattern)
            ("request id", "Request ID contains invalid characters"), // space
            ("request@123", "Request ID contains invalid characters"), // @
            ("request/123", "Request ID contains invalid characters"), // slash
            ("request#123", "Request ID contains invalid characters"), // hash
            ("request+123", "Request ID contains invalid characters"), // plus
            ("request%123", "Request ID contains invalid characters"), // percent
            ("request&123", "Request ID contains invalid characters"), // ampersand
            ("request*123", "Request ID contains invalid characters"), // asterisk
            ("request(123", "Request ID contains invalid characters"), // parentheses
            ("request)123", "Request ID contains invalid characters"), // parentheses
            ("request[123", "Request ID contains invalid characters"), // brackets
            ("request]123", "Request ID contains invalid characters"), // brackets
            ("request{123", "Request ID contains invalid characters"), // braces
            ("request}123", "Request ID contains invalid characters"), // braces
            ("request$123", "Request ID contains invalid characters"), // dollar
            ("request!123", "Request ID contains invalid characters"), // exclamation
            // AWS reserved prefix (case insensitive)
            ("aws-service", "Request ID cannot start with 'aws-'"),
            ("AWS-Service", "Request ID cannot start with 'aws-'"),
            ("Aws-test", "Request ID cannot start with 'aws-'"),
            ("awS-test", "Request ID cannot start with 'aws-'"),
            // Starting with non-alphanumeric
            (
                "-request",
                "Request ID must start with an alphanumeric character",
            ),
            (
                "_request",
                "Request ID must start with an alphanumeric character",
            ),
            (
                ".request",
                "Request ID must start with an alphanumeric character",
            ),
            ("!request", "Request ID contains invalid characters."),
            // Ending with special characters
            (
                "request-",
                "Request ID cannot end with hyphens, underscores, or dots",
            ),
            (
                "request_",
                "Request ID cannot end with hyphens, underscores, or dots",
            ),
            (
                "request.",
                "Request ID cannot end with hyphens, underscores, or dots",
            ),
            // Consecutive special characters
            (
                "request--123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request__123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request..123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request-_123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request_-123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request-.123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request._123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request_.123",
                "Request ID cannot contain consecutive special characters",
            ),
            (
                "request.-123",
                "Request ID cannot contain consecutive special characters",
            ),
        ];

        for (id, expected_error_substring) in test_cases {
            let result = validate_request_id(id);
            assert!(
                result.is_err(),
                "Expected '{}' to be invalid, but validation passed",
                id
            );

            let error_msg = result.unwrap_err();
            assert!(
                error_msg.contains(expected_error_substring),
                "Expected error for '{}' to contain '{}', but got: '{}'",
                id,
                expected_error_substring,
                error_msg
            );
        }
    }

    #[derive(Debug, Clone)]
    struct S3PutOperation {
        request_id: String,
        bucket: String,
        object_name: String,
        content_size: usize,
    }

    #[derive(Debug, Clone)]
    struct S3DeleteOperation {
        request_id: String,
        bucket: String,
        object_name: String,
    }

    #[derive(Debug, Clone)]
    struct CreateVocabularyOperation {
        vocabulary_name: String,
        language_code: String,
        phrases: Vec<String>,
    }

    #[derive(Debug, Clone)]
    struct StartTranscriptionOperation {
        job_name: String,
        media_uri: String,
        audio_config: AudioConfig,
        transcription_config: Option<TranscriptionConfig>,
        vocabulary_name: Option<String>,
    }

    #[derive(Debug, Clone)]
    struct DownloadTranscriptOperation {
        job_name: String,
        transcript_uri: String,
    }

    struct MockS3Client {
        pub put_object_responses: RefCell<VecDeque<Result<(), Error>>>,
        pub delete_object_responses: RefCell<VecDeque<Result<(), Error>>>,
        pub captured_put_operations: RefCell<Vec<S3PutOperation>>,
        pub captured_delete_operations: RefCell<Vec<S3DeleteOperation>>,
    }

    #[allow(unused)]
    impl MockS3Client {
        pub fn new() -> Self {
            Self {
                put_object_responses: RefCell::new(VecDeque::new()),
                delete_object_responses: RefCell::new(VecDeque::new()),
                captured_put_operations: RefCell::new(Vec::new()),
                captured_delete_operations: RefCell::new(Vec::new()),
            }
        }

        pub fn expect_put_object_response(&self, response: Result<(), Error>) {
            self.put_object_responses.borrow_mut().push_back(response);
        }

        pub fn expect_delete_object_response(&self, response: Result<(), Error>) {
            self.delete_object_responses
                .borrow_mut()
                .push_back(response);
        }

        pub fn get_captured_put_operations(&self) -> Ref<Vec<S3PutOperation>> {
            self.captured_put_operations.borrow()
        }

        pub fn get_captured_delete_operations(&self) -> Ref<Vec<S3DeleteOperation>> {
            self.captured_delete_operations.borrow()
        }

        pub fn clear_captured_operations(&self) {
            self.captured_put_operations.borrow_mut().clear();
            self.captured_delete_operations.borrow_mut().clear();
        }
    }
    #[allow(unused)]
    impl S3Service for MockS3Client {
        async fn put_object(
            &self,
            request_id: &str,
            bucket: &str,
            object_name: &str,
            content: Bytes,
        ) -> Result<(), Error> {
            self.captured_put_operations
                .borrow_mut()
                .push(S3PutOperation {
                    request_id: request_id.to_string(),
                    bucket: bucket.to_string(),
                    object_name: object_name.to_string(),
                    content_size: content.len(),
                });

            self.put_object_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    request_id.to_string(),
                    client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }

        async fn delete_object(
            &self,
            request_id: &str,
            bucket: &str,
            object_name: &str,
        ) -> Result<(), Error> {
            self.captured_delete_operations
                .borrow_mut()
                .push(S3DeleteOperation {
                    request_id: request_id.to_string(),
                    bucket: bucket.to_string(),
                    object_name: object_name.to_string(),
                });

            self.delete_object_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    request_id.to_string(),
                    client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }
    }

    struct MockTranscribeClient {
        pub create_vocabulary_responses: RefCell<VecDeque<Result<CreateVocabularyResponse, Error>>>,
        pub get_vocabulary_responses: RefCell<VecDeque<Result<GetVocabularyResponse, Error>>>,
        pub start_transcription_responses:
            RefCell<VecDeque<Result<StartTranscriptionJobResponse, Error>>>,
        pub get_transcription_responses:
            RefCell<VecDeque<Result<GetTranscriptionJobResponse, Error>>>,
        pub download_transcript_responses: RefCell<VecDeque<Result<TranscribeOutput, Error>>>,
        pub delete_vocabulary_responses: RefCell<VecDeque<Result<(), Error>>>,
        pub captured_create_vocabulary: RefCell<Vec<CreateVocabularyOperation>>,
        pub captured_get_vocabulary: RefCell<Vec<String>>,
        pub captured_start_transcription: RefCell<Vec<StartTranscriptionOperation>>,
        pub captured_get_transcription: RefCell<Vec<String>>,
        pub captured_download_transcript: RefCell<Vec<DownloadTranscriptOperation>>,
        pub captured_delete_vocabulary: RefCell<Vec<String>>,
    }

    #[allow(unused)]
    impl MockTranscribeClient {
        pub fn new() -> Self {
            Self {
                create_vocabulary_responses: RefCell::new(VecDeque::new()),
                get_vocabulary_responses: RefCell::new(VecDeque::new()),
                start_transcription_responses: RefCell::new(VecDeque::new()),
                get_transcription_responses: RefCell::new(VecDeque::new()),
                download_transcript_responses: RefCell::new(VecDeque::new()),
                delete_vocabulary_responses: RefCell::new(VecDeque::new()),
                captured_create_vocabulary: RefCell::new(Vec::new()),
                captured_get_vocabulary: RefCell::new(Vec::new()),
                captured_start_transcription: RefCell::new(Vec::new()),
                captured_get_transcription: RefCell::new(Vec::new()),
                captured_download_transcript: RefCell::new(Vec::new()),
                captured_delete_vocabulary: RefCell::new(Vec::new()),
            }
        }

        pub fn expect_create_vocabulary_response(
            &self,
            response: Result<CreateVocabularyResponse, Error>,
        ) {
            self.create_vocabulary_responses
                .borrow_mut()
                .push_back(response);
        }

        pub fn expect_get_vocabulary_response(
            &self,
            response: Result<GetVocabularyResponse, Error>,
        ) {
            self.get_vocabulary_responses
                .borrow_mut()
                .push_back(response);
        }

        pub fn expect_start_transcription_response(
            &self,
            response: Result<StartTranscriptionJobResponse, Error>,
        ) {
            self.start_transcription_responses
                .borrow_mut()
                .push_back(response);
        }

        pub fn expect_get_transcription_response(
            &self,
            response: Result<GetTranscriptionJobResponse, Error>,
        ) {
            self.get_transcription_responses
                .borrow_mut()
                .push_back(response);
        }

        pub fn expect_download_transcript_response(
            &self,
            response: Result<TranscribeOutput, Error>,
        ) {
            self.download_transcript_responses
                .borrow_mut()
                .push_back(response);
        }

        pub fn expect_delete_vocabulary_response(&self, response: Result<(), Error>) {
            self.delete_vocabulary_responses
                .borrow_mut()
                .push_back(response);
        }

        pub fn get_captured_create_vocabulary(&self) -> Ref<Vec<CreateVocabularyOperation>> {
            self.captured_create_vocabulary.borrow()
        }

        pub fn get_captured_start_transcription(&self) -> Ref<Vec<StartTranscriptionOperation>> {
            self.captured_start_transcription.borrow()
        }

        pub fn get_captured_download_transcript(&self) -> Ref<Vec<DownloadTranscriptOperation>> {
            self.captured_download_transcript.borrow()
        }

        pub fn clear_captured_operations(&self) {
            self.captured_create_vocabulary.borrow_mut().clear();
            self.captured_get_vocabulary.borrow_mut().clear();
            self.captured_start_transcription.borrow_mut().clear();
            self.captured_get_transcription.borrow_mut().clear();
            self.captured_download_transcript.borrow_mut().clear();
            self.captured_delete_vocabulary.borrow_mut().clear();
        }
    }

    impl TranscribeService for MockTranscribeClient {
        async fn create_vocabulary(
            &self,
            vocabulary_name: String,
            language_code: String,
            phrases: Vec<String>,
        ) -> Result<CreateVocabularyResponse, Error> {
            self.captured_create_vocabulary
                .borrow_mut()
                .push(CreateVocabularyOperation {
                    vocabulary_name: vocabulary_name.clone(),
                    language_code: language_code.clone(),
                    phrases: phrases.clone(),
                });

            self.create_vocabulary_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    vocabulary_name.clone(),
                    golem_stt::client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }

        async fn get_vocabulary(
            &self,
            vocabulary_name: &str,
        ) -> Result<GetVocabularyResponse, Error> {
            self.captured_get_vocabulary
                .borrow_mut()
                .push(vocabulary_name.to_string());

            self.get_vocabulary_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    vocabulary_name.to_string(),
                    golem_stt::client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }

        async fn wait_for_vocabulary_ready<RT: AsyncRuntime>(
            &self,
            _runtime: &RT,
            _request_id: &str,
            _max_wait_time: std::time::Duration,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn delete_vocabulary(&self, vocabulary_name: &str) -> Result<(), Error> {
            self.captured_delete_vocabulary
                .borrow_mut()
                .push(vocabulary_name.to_string());

            self.delete_vocabulary_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    vocabulary_name.to_string(),
                    golem_stt::client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }

        async fn start_transcription_job(
            &self,
            transcription_job_name: String,
            media_file_uri: String,
            audio_config: AudioConfig,
            transcription_config: Option<TranscriptionConfig>,
            vocabulary_name: Option<String>,
        ) -> Result<StartTranscriptionJobResponse, Error> {
            self.captured_start_transcription
                .borrow_mut()
                .push(StartTranscriptionOperation {
                    job_name: transcription_job_name.clone(),
                    media_uri: media_file_uri.clone(),
                    audio_config: audio_config.clone(),
                    transcription_config: transcription_config.clone(),
                    vocabulary_name: vocabulary_name.clone(),
                });

            self.start_transcription_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    transcription_job_name.clone(),
                    golem_stt::client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }

        async fn get_transcription_job(
            &self,
            transcription_job_name: String,
        ) -> Result<GetTranscriptionJobResponse, Error> {
            self.captured_get_transcription
                .borrow_mut()
                .push(transcription_job_name.clone());

            self.get_transcription_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    transcription_job_name.clone(),
                    golem_stt::client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }

        async fn wait_for_transcription_job_completion<RT: AsyncRuntime>(
            &self,
            _runtime: &RT,
            transcription_job_name: &str,
            _max_wait_time: std::time::Duration,
        ) -> Result<GetTranscriptionJobResponse, Error> {
            self.get_transcription_job(transcription_job_name.to_string())
                .await
        }

        async fn download_transcript_json(
            &self,
            transcription_job_name: &str,
            transcript_uri: &str,
        ) -> Result<TranscribeOutput, Error> {
            self.captured_download_transcript
                .borrow_mut()
                .push(DownloadTranscriptOperation {
                    job_name: transcription_job_name.to_string(),
                    transcript_uri: transcript_uri.to_string(),
                });

            self.download_transcript_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err((
                    transcription_job_name.to_string(),
                    golem_stt::client::Error::Generic("unexpected error".to_string()),
                )
                    .into()))
        }
    }

    struct MockRuntime;

    impl AsyncRuntime for MockRuntime {
        async fn sleep(&self, _duration: std::time::Duration) {}
    }

    fn create_mock_transcribe_api() -> TranscribeApi<MockS3Client, MockTranscribeClient, MockRuntime>
    {
        TranscribeApi {
            bucket_name: "test-bucket".to_string(),
            s3_client: MockS3Client::new(),
            transcribe_client: MockTranscribeClient::new(),
            runtime: MockRuntime,
        }
    }

    #[test]
    fn test_transcribe_audio_invalid_request_id_returns_error() {
        let api = create_mock_transcribe_api();

        let request = TranscriptionRequest {
            request_id: "invalid request id".to_string(), // spaces are invalid
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: None,
        };

        let result = block_on(|_| async { api.transcribe_audio(request).await });
        assert!(result.is_err());

        if let Err(Error::APIBadRequest { provider_error, .. }) = result {
            assert!(provider_error.contains("Invalid request ID"));
        } else {
            panic!("Expected APIBadRequest error");
        }
    }

    #[test]
    fn test_transcribe_audio_vocabulary_without_language_returns_error() {
        let api = create_mock_transcribe_api();

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: Some(TranscriptionConfig {
                language: None, // No language specified
                model: None,
                enable_speaker_diarization: false,
                vocabulary: vec!["word1".to_string(), "word2".to_string()], // But vocabulary provided
            }),
        };

        let result = block_on(|_| async { api.transcribe_audio(request).await });
        assert!(result.is_err());

        if let Err(Error::APIBadRequest { provider_error, .. }) = result {
            assert!(provider_error
                .contains("Vocabulary can only be used when a specific language is provided"));
        } else {
            panic!("Expected APIBadRequest error");
        }
    }

    #[test]
    fn test_transcribe_audio_model_without_language_returns_error() {
        let api = create_mock_transcribe_api();

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: Some(TranscriptionConfig {
                language: None,                             // No language specified
                model: Some("en-US_Telephony".to_string()), // But model provided
                enable_speaker_diarization: false,
                vocabulary: vec![],
            }),
        };

        let result = block_on(|_| async { api.transcribe_audio(request).await });
        assert!(result.is_err());

        if let Err(Error::APIBadRequest { provider_error, .. }) = result {
            assert!(provider_error
                .contains("Model settings can only be used when a specific language is provided"));
        } else {
            panic!("Expected APIBadRequest error");
        }
    }

    #[test]
    fn test_transcribe_audio_uploads_to_s3() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_start_transcription_response(Ok(StartTranscriptionJobResponse {
                transcription_job: TranscriptionJob {
                    transcription_job_name: "test-123".to_string(),
                    transcription_job_status: "COMPLETED".to_string(),
                    language_code: None,
                    media: None,
                    media_format: None,
                    media_sample_rate_hertz: None,
                    creation_time: None,
                    completion_time: None,
                    start_time: None,
                    failure_reason: None,
                    settings: None,
                    transcript: Some(Transcript {
                        transcript_file_uri: Some(
                            "https://example.com/transcript.json".to_string(),
                        ),
                        redacted_transcript_file_uri: None,
                    }),
                    tags: None,
                },
            }));
        api.transcribe_client
            .expect_download_transcript_response(Ok(TranscribeOutput {
                job_name: "test-123".to_string(),
                account_id: "123456789".to_string(),
                results: TranscribeResults {
                    transcripts: vec![TranscriptText {
                        transcript: "Hello world".to_string(),
                    }],
                    speaker_labels: None,
                    items: vec![],
                },
                status: "COMPLETED".to_string(),
            }));
        api.transcribe_client
            .expect_delete_vocabulary_response(Ok(()));
        api.s3_client.expect_delete_object_response(Ok(()));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio data"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: None,
        };

        let _ = block_on(|_| async { api.transcribe_audio(request).await });

        let captured_puts = api.s3_client.get_captured_put_operations();
        assert_eq!(captured_puts.len(), 1);
        let put_op = &captured_puts[0];
        assert_eq!(put_op.request_id, "test-123");
        assert_eq!(put_op.bucket, "test-bucket");
        assert_eq!(put_op.object_name, "test-123/audio.wav");
        assert_eq!(put_op.content_size, 15);
    }

    #[test]
    fn test_transcribe_audio_creates_vocabulary_when_provided() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_create_vocabulary_response(Ok(CreateVocabularyResponse {
                vocabulary_name: "test-123".to_string(),
                language_code: "en-US".to_string(),
                vocabulary_state: "READY".to_string(),
                last_modified_time: None,
                failure_reason: None,
            }));
        api.transcribe_client
            .expect_start_transcription_response(Ok(StartTranscriptionJobResponse {
                transcription_job: TranscriptionJob {
                    transcription_job_name: "test-123".to_string(),
                    transcription_job_status: "COMPLETED".to_string(),
                    language_code: None,
                    media: None,
                    media_format: None,
                    media_sample_rate_hertz: None,
                    creation_time: None,
                    completion_time: None,
                    start_time: None,
                    failure_reason: None,
                    settings: None,
                    transcript: Some(Transcript {
                        transcript_file_uri: Some(
                            "https://example.com/transcript.json".to_string(),
                        ),
                        redacted_transcript_file_uri: None,
                    }),
                    tags: None,
                },
            }));
        api.transcribe_client
            .expect_download_transcript_response(Ok(TranscribeOutput {
                job_name: "test-123".to_string(),
                account_id: "123456789".to_string(),
                results: TranscribeResults {
                    transcripts: vec![TranscriptText {
                        transcript: "Hello world".to_string(),
                    }],
                    speaker_labels: None,
                    items: vec![],
                },
                status: "COMPLETED".to_string(),
            }));
        api.transcribe_client
            .expect_delete_vocabulary_response(Ok(()));
        api.s3_client.expect_delete_object_response(Ok(()));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::mp3,
                channels: Some(1),
            },
            transcription_config: Some(TranscriptionConfig {
                language: Some("en-US".to_string()),
                model: None,
                enable_speaker_diarization: false,
                vocabulary: vec!["custom".to_string(), "words".to_string()],
            }),
        };

        let _ = block_on(|_| async { api.transcribe_audio(request).await });

        let captured_vocabulary = api.transcribe_client.get_captured_create_vocabulary();
        assert_eq!(captured_vocabulary.len(), 1);
        let vocab_op = &captured_vocabulary[0];
        assert_eq!(vocab_op.vocabulary_name, "test-123");
        assert_eq!(vocab_op.language_code, "en-US");
        assert_eq!(
            vocab_op.phrases,
            vec!["custom".to_string(), "words".to_string()]
        );
    }

    #[test]
    fn test_transcribe_audio_starts_transcription_job() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_start_transcription_response(Ok(StartTranscriptionJobResponse {
                transcription_job: TranscriptionJob {
                    transcription_job_name: "test-123".to_string(),
                    transcription_job_status: "COMPLETED".to_string(),
                    language_code: None,
                    media: None,
                    media_format: None,
                    media_sample_rate_hertz: None,
                    creation_time: None,
                    completion_time: None,
                    start_time: None,
                    failure_reason: None,
                    settings: None,
                    transcript: Some(Transcript {
                        transcript_file_uri: Some(
                            "https://example.com/transcript.json".to_string(),
                        ),
                        redacted_transcript_file_uri: None,
                    }),
                    tags: None,
                },
            }));
        api.transcribe_client
            .expect_download_transcript_response(Ok(TranscribeOutput {
                job_name: "test-123".to_string(),
                account_id: "123456789".to_string(),
                results: TranscribeResults {
                    transcripts: vec![TranscriptText {
                        transcript: "Hello world".to_string(),
                    }],
                    speaker_labels: None,
                    items: vec![],
                },
                status: "COMPLETED".to_string(),
            }));
        api.transcribe_client
            .expect_delete_vocabulary_response(Ok(()));
        api.s3_client.expect_delete_object_response(Ok(()));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::flac,
                channels: Some(2),
            },
            transcription_config: Some(TranscriptionConfig {
                language: Some("es-ES".to_string()),
                model: Some("en-US_Telephony".to_string()),
                enable_speaker_diarization: true,
                vocabulary: vec![],
            }),
        };

        let _ = block_on(|_| async { api.transcribe_audio(request).await });

        let captured_transcription = api.transcribe_client.get_captured_start_transcription();
        assert_eq!(captured_transcription.len(), 1);
        let transcription_op = &captured_transcription[0];
        assert_eq!(transcription_op.job_name, "test-123");
        assert_eq!(
            transcription_op.media_uri,
            "s3://test-bucket/test-123/audio.flac"
        );
        assert_eq!(transcription_op.audio_config.format.to_string(), "flac");
        assert_eq!(transcription_op.audio_config.channels, Some(2));
        assert!(
            transcription_op
                .transcription_config
                .as_ref()
                .unwrap()
                .enable_speaker_diarization
        );
        assert_eq!(transcription_op.vocabulary_name, None);
    }

    #[test]
    fn test_transcribe_audio_downloads_transcript_and_returns_response() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_start_transcription_response(Ok(StartTranscriptionJobResponse {
                transcription_job: TranscriptionJob {
                    transcription_job_name: "test-123".to_string(),
                    transcription_job_status: "COMPLETED".to_string(),
                    language_code: None,
                    media: None,
                    media_format: None,
                    media_sample_rate_hertz: None,
                    creation_time: None,
                    completion_time: None,
                    start_time: None,
                    failure_reason: None,
                    settings: None,
                    transcript: Some(Transcript {
                        transcript_file_uri: Some(
                            "https://example.com/transcript.json".to_string(),
                        ),
                        redacted_transcript_file_uri: None,
                    }),
                    tags: None,
                },
            }));
        api.transcribe_client
            .expect_download_transcript_response(Ok(TranscribeOutput {
                job_name: "test-123".to_string(),
                account_id: "123456789".to_string(),
                results: TranscribeResults {
                    transcripts: vec![TranscriptText {
                        transcript: "Hello world".to_string(),
                    }],
                    speaker_labels: None,
                    items: vec![],
                },
                status: "COMPLETED".to_string(),
            }));
        api.transcribe_client
            .expect_delete_vocabulary_response(Ok(()));
        api.s3_client.expect_delete_object_response(Ok(()));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio data"),
            audio_config: AudioConfig {
                format: AudioFormat::ogg,
                channels: Some(1),
            },
            transcription_config: Some(TranscriptionConfig {
                language: Some("fr-FR".to_string()),
                model: None,
                enable_speaker_diarization: false,
                vocabulary: vec![],
            }),
        };

        let response = block_on(|_| async { api.transcribe_audio(request).await.unwrap() });

        assert_eq!(response.audio_size_bytes, 15);
        assert_eq!(response.language, "fr-FR");

        let captured_downloads = api.transcribe_client.get_captured_download_transcript();
        assert_eq!(captured_downloads.len(), 1);
        let download_op = &captured_downloads[0];
        assert_eq!(download_op.job_name, "test-123");
        assert_eq!(
            download_op.transcript_uri,
            "https://example.com/transcript.json"
        );
    }

    #[test]
    fn test_transcribe_audio_cleans_up_resources() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_create_vocabulary_response(Ok(CreateVocabularyResponse {
                vocabulary_name: "test-123".to_string(),
                language_code: "en-US".to_string(),
                vocabulary_state: "READY".to_string(),
                last_modified_time: None,
                failure_reason: None,
            }));
        api.transcribe_client
            .expect_start_transcription_response(Ok(StartTranscriptionJobResponse {
                transcription_job: TranscriptionJob {
                    transcription_job_name: "test-123".to_string(),
                    transcription_job_status: "COMPLETED".to_string(),
                    language_code: None,
                    media: None,
                    media_format: None,
                    media_sample_rate_hertz: None,
                    creation_time: None,
                    completion_time: None,
                    start_time: None,
                    failure_reason: None,
                    settings: None,
                    transcript: Some(Transcript {
                        transcript_file_uri: Some(
                            "https://example.com/transcript.json".to_string(),
                        ),
                        redacted_transcript_file_uri: None,
                    }),
                    tags: None,
                },
            }));
        api.transcribe_client
            .expect_download_transcript_response(Ok(TranscribeOutput {
                job_name: "test-123".to_string(),
                account_id: "123456789".to_string(),
                results: TranscribeResults {
                    transcripts: vec![TranscriptText {
                        transcript: "Hello world".to_string(),
                    }],
                    speaker_labels: None,
                    items: vec![],
                },
                status: "COMPLETED".to_string(),
            }));
        api.transcribe_client
            .expect_delete_vocabulary_response(Ok(()));
        api.s3_client.expect_delete_object_response(Ok(()));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: Some(TranscriptionConfig {
                language: Some("en-US".to_string()),
                model: None,
                enable_speaker_diarization: false,
                vocabulary: vec!["word1".to_string()],
            }),
        };

        let _ = block_on(|_| async { api.transcribe_audio(request).await });

        // Check vocabulary was deleted
        let captured_vocab_deletes = api.transcribe_client.captured_delete_vocabulary.borrow();
        assert_eq!(captured_vocab_deletes.len(), 1);
        assert_eq!(captured_vocab_deletes[0], "test-123");

        // Check S3 object was deleted
        let captured_deletes = api.s3_client.get_captured_delete_operations();
        assert_eq!(captured_deletes.len(), 1);
        let delete_op = &captured_deletes[0];
        assert_eq!(delete_op.request_id, "test-123");
        assert_eq!(delete_op.bucket, "test-bucket");
        assert_eq!(delete_op.object_name, "test-123/audio.wav");
    }

    #[test]
    fn test_transcribe_audio_s3_upload_failure() {
        let api = create_mock_transcribe_api();

        api.s3_client
            .expect_put_object_response(Err(Error::APIInternalServerError {
                request_id: "test-123".to_string(),
                provider_error: "S3 upload failed".to_string(),
            }));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: None,
        };

        let result = block_on(|_| async { api.transcribe_audio(request).await });
        assert!(result.is_err());

        if let Err(Error::APIInternalServerError { provider_error, .. }) = result {
            assert!(provider_error.contains("S3 upload failed"));
        } else {
            panic!("Expected APIInternalServerError");
        }
    }

    #[test]
    fn test_transcribe_audio_vocabulary_creation_failure() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_create_vocabulary_response(Ok(CreateVocabularyResponse {
                vocabulary_name: "test-123".to_string(),
                language_code: "en-US".to_string(),
                vocabulary_state: "FAILED".to_string(),
                last_modified_time: None,
                failure_reason: Some("Invalid vocabulary words".to_string()),
            }));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: Some(TranscriptionConfig {
                language: Some("en-US".to_string()),
                model: None,
                enable_speaker_diarization: false,
                vocabulary: vec!["invalid".to_string()],
            }),
        };

        let result = block_on(|_| async { api.transcribe_audio(request).await });
        assert!(result.is_err());

        if let Err(Error::APIBadRequest { provider_error, .. }) = result {
            assert!(provider_error.contains("Vocabulary creation failed"));
            assert!(provider_error.contains("Invalid vocabulary words"));
        } else {
            panic!("Expected APIBadRequest error");
        }
    }

    #[test]
    fn test_transcribe_audio_transcription_job_failure() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_start_transcription_response(Ok(StartTranscriptionJobResponse {
                transcription_job: TranscriptionJob {
                    transcription_job_name: "test-123".to_string(),
                    transcription_job_status: "FAILED".to_string(),
                    language_code: None,
                    media: None,
                    media_format: None,
                    media_sample_rate_hertz: None,
                    creation_time: None,
                    completion_time: None,
                    start_time: None,
                    failure_reason: Some("Audio format not supported".to_string()),
                    settings: None,
                    transcript: None,
                    tags: None,
                },
            }));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: None,
        };

        let result = block_on(|_| async { api.transcribe_audio(request).await });
        assert!(result.is_err());

        if let Err(Error::APIBadRequest { provider_error, .. }) = result {
            assert!(provider_error.contains("Transcription job creation failed"));
            assert!(provider_error.contains("Audio format not supported"));
        } else {
            panic!("Expected APIBadRequest error");
        }
    }

    #[test]
    fn test_transcribe_audio_transcript_download_failure() {
        let api = create_mock_transcribe_api();

        api.s3_client.expect_put_object_response(Ok(()));
        api.transcribe_client
            .expect_start_transcription_response(Ok(StartTranscriptionJobResponse {
                transcription_job: TranscriptionJob {
                    transcription_job_name: "test-123".to_string(),
                    transcription_job_status: "COMPLETED".to_string(),
                    language_code: None,
                    media: None,
                    media_format: None,
                    media_sample_rate_hertz: None,
                    creation_time: None,
                    completion_time: None,
                    start_time: None,
                    failure_reason: None,
                    settings: None,
                    transcript: Some(Transcript {
                        transcript_file_uri: Some(
                            "https://example.com/transcript.json".to_string(),
                        ),
                        redacted_transcript_file_uri: None,
                    }),
                    tags: None,
                },
            }));
        api.transcribe_client
            .expect_download_transcript_response(Err(Error::APINotFound {
                request_id: "test-123".to_string(),
                provider_error: "Transcript file not found".to_string(),
            }));

        let request = TranscriptionRequest {
            request_id: "test-123".to_string(),
            audio: Bytes::from("test audio"),
            audio_config: AudioConfig {
                format: AudioFormat::wav,
                channels: Some(1),
            },
            transcription_config: None,
        };

        let result = block_on(|_| async { api.transcribe_audio(request).await });
        assert!(result.is_err());

        if let Err(Error::APINotFound { provider_error, .. }) = result {
            assert!(provider_error.contains("Transcript file not found"));
        } else {
            panic!("Expected APINotFound error");
        }
    }
}
