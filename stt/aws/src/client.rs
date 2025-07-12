use std::{rc::Rc, sync::Arc, time::Duration};

use crate::aws::{S3Client, TranscribeClient};
use golem_stt::{
    client::{HttpClient, SttProviderClient},
    error::Error,
    runtime::AsyncRuntime,
};

use bytes::Bytes;
use log::trace;
use serde::{Deserialize, Serialize};

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

pub struct TranscribeApi<HC: HttpClient, RT: AsyncRuntime> {
    bucket_name: Arc<str>,
    s3_client: S3Client<HC>,
    transcribe_client: TranscribeClient<HC>,
    runtime: RT,
}

impl<HC: HttpClient, RT: AsyncRuntime>
    SttProviderClient<TranscriptionRequest, TranscriptionResponse, Error>
    for TranscribeApi<HC, RT>
{
    async fn transcribe_audio(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionResponse, Error> {
        trace!("Sending request to AWS Transcribe API: {request:?}");

        let request_id: Rc<str> = Rc::from(request.request_id);

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
            .put_object(self.bucket_name.as_ref(), &object_key, request.audio)
            .await
            .map_err(|err| Error::Client(request_id.to_string(), err))?;

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
                    .await
                    .map_err(|err| Error::Client(request_id.to_string(), err))?;

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
                format!("s3://{}/{object_key}", self.bucket_name),
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

                todo!()
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

// https://docs.aws.amazon.com/transcribe/latest/dg/how-input.html#how-output
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeOutput {
    pub job_name: String,
    pub account_id: String,
    pub results: TranscribeResults,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeResults {
    pub transcripts: Vec<TranscriptText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_labels: Option<SpeakerLabels>,
    pub items: Vec<TranscribeItem>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptText {
    pub transcript: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerLabels {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_label: Option<String>,
    pub speakers: i32,
    pub segments: Vec<SpeakerSegment>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerSegment {
    pub start_time: String,
    pub speaker_label: String,
    pub end_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<SpeakerItem>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerItem {
    pub start_time: String,
    pub speaker_label: String,
    pub end_time: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_label: Option<String>,
    pub alternatives: Vec<TranscribeAlternative>,
    #[serde(rename = "type")]
    pub item_type: String, // "pronunciation" or "punctuation"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary_filter_match: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeAlternative {
    pub confidence: String, // Note: AWS returns this as a string, not a number
    pub content: String,
}

// Helper struct for working with word metadata
#[derive(Debug, Clone)]
pub struct WordMetadata {
    pub word: String,
    pub confidence: f64,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub speaker_label: Option<String>,
}

#[cfg(test)]
mod tests {
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
}
