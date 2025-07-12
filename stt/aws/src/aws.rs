use bytes::Bytes;
use chrono::{DateTime, Utc};
use golem_stt::{
    client::{self, HttpClient},
    runtime::AsyncRuntime,
};
use hmac::{Hmac, Mac};
use http::{HeaderMap, HeaderValue, Request};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt::{self},
    sync::Arc,
    time::Duration,
};

use crate::{
    client::{AudioConfig, TranscribeOutput, TranscriptionConfig},
    error::Error,
};

type HmacSha256 = Hmac<Sha256>;

pub struct AwsSignatureV4 {
    access_key: String,
    secret_key: String,
    region: String,
    service: String,
}

pub enum AwsService {
    S3,
    Transcribe,
}

impl fmt::Display for AwsService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AwsService::S3 => write!(f, "s3"),
            AwsService::Transcribe => write!(f, "transcribe"),
        }
    }
}

// Percent-encoding set for URI paths
// Why this is needed see here https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html
// AWS uri encoding has special characters that need to be percent-encoded
const URI_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// AWS-specific percent-encoding set for query strings
const QUERY_ENCODE_SET: &AsciiSet = &URI_ENCODE_SET.add(b'=').add(b'&').add(b'+');

impl AwsSignatureV4 {
    pub fn new(
        access_key: String,
        secret_key: String,
        region: String,
        service: AwsService,
    ) -> Self {
        Self {
            access_key,
            secret_key,
            region,
            service: service.to_string(),
        }
    }

    pub fn for_s3(access_key: String, secret_key: String, region: String) -> Self {
        Self::new(access_key, secret_key, region, AwsService::S3)
    }

    pub fn for_transcribe(access_key: String, secret_key: String, region: String) -> Self {
        Self::new(access_key, secret_key, region, AwsService::Transcribe)
    }

    pub fn get_region(&self) -> &str {
        &self.region
    }

    pub fn sign_request(
        &self,
        request: Request<Bytes>,
        timestamp: DateTime<Utc>,
    ) -> Result<Request<Bytes>, Error> {
        let (mut parts, body) = request.into_parts();

        let date_stamp = timestamp.format("%Y%m%d").to_string();
        let amz_date = timestamp.format("%Y%m%dT%H%M%SZ").to_string();

        parts
            .headers
            .insert("x-amz-date", HeaderValue::from_str(&amz_date)?);

        let content_sha256 = self.hash_payload(body.as_ref());
        parts.headers.insert(
            "x-amz-content-sha256",
            HeaderValue::from_str(&content_sha256)?,
        );

        if !parts.headers.contains_key("host") {
            if let Some(host) = parts.uri.host() {
                let host_header = if let Some(port) = parts.uri.port_u16() {
                    format!("{}:{}", host, port)
                } else {
                    host.to_string()
                };
                parts
                    .headers
                    .insert("host", HeaderValue::from_str(&host_header)?);
            }
        }

        let canonical_request = self.create_canonical_request(
            &parts.method,
            &parts.uri,
            &parts.headers,
            &content_sha256,
        );

        let string_to_sign = self.create_string_to_sign(&canonical_request, &amz_date, &date_stamp);

        let signature = self.calculate_signature(&string_to_sign, &date_stamp)?;

        let signed_headers = self.get_signed_headers(&parts.headers);
        let credential = format!(
            "{}/{}/{}/{}/aws4_request",
            self.access_key, date_stamp, self.region, self.service
        );
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}, SignedHeaders={}, Signature={}",
            credential, signed_headers, signature
        );

        parts
            .headers
            .insert("authorization", HeaderValue::from_str(&authorization)?);

        Ok(Request::from_parts(parts, body))
    }

    fn create_canonical_request(
        &self,
        method: &http::Method,
        uri: &http::Uri,
        headers: &HeaderMap,
        content_sha256: &str,
    ) -> String {
        let canonical_uri = self.canonical_uri(uri.path());

        let canonical_query_string = self.canonical_query_string(uri.query().unwrap_or(""));

        let canonical_headers = self.canonical_headers(headers);

        let signed_headers = self.get_signed_headers(headers);

        let hashed_payload = content_sha256;

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method.as_str().to_uppercase(),
            canonical_uri,
            canonical_query_string,
            canonical_headers,
            signed_headers,
            hashed_payload
        );

        canonical_request
    }

    fn canonical_uri(&self, path: &str) -> String {
        if path.is_empty() {
            "/".to_string()
        } else {
            // URI encode each segment
            let segments: Vec<String> = path
                .split('/')
                .map(|segment| utf8_percent_encode(segment, URI_ENCODE_SET).to_string())
                .collect();
            segments.join("/")
        }
    }

    fn canonical_query_string(&self, query: &str) -> String {
        if query.is_empty() {
            return String::new();
        }

        let mut params: Vec<(String, String)> = query
            .split('&')
            .filter_map(|param| {
                if let Some(eq_pos) = param.find('=') {
                    let key = &param[..eq_pos];
                    let value = &param[eq_pos + 1..];
                    Some((
                        utf8_percent_encode(key, QUERY_ENCODE_SET).to_string(),
                        utf8_percent_encode(value, QUERY_ENCODE_SET).to_string(),
                    ))
                } else {
                    Some((
                        utf8_percent_encode(param, QUERY_ENCODE_SET).to_string(),
                        String::new(),
                    ))
                }
            })
            .collect();

        params.sort_by(|a, b| a.0.cmp(&b.0));

        params
            .into_iter()
            .map(|(key, value)| {
                if value.is_empty() {
                    key
                } else {
                    format!("{}={}", key, value)
                }
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    fn canonical_headers(&self, headers: &HeaderMap) -> String {
        let mut canonical_headers = String::new();

        let mut sorted_headers: Vec<_> = headers
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_lowercase(),
                    value.to_str().unwrap_or("").trim(),
                )
            })
            .collect();
        sorted_headers.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, value) in sorted_headers {
            canonical_headers.push_str(&format!("{}:{}\n", name, value));
        }

        canonical_headers
    }

    fn get_signed_headers(&self, headers: &HeaderMap) -> String {
        let mut signed_headers: Vec<String> = headers
            .keys()
            .map(|key| key.as_str().to_lowercase())
            .collect();
        signed_headers.sort();
        signed_headers.join(";")
    }

    fn create_string_to_sign(
        &self,
        canonical_request: &str,
        amz_date: &str,
        date_stamp: &str,
    ) -> String {
        let algorithm = "AWS4-HMAC-SHA256";
        let credential_scope = format!(
            "{}/{}/{}/aws4_request",
            date_stamp, self.region, self.service
        );

        let hashed_canonical_request = self.hash_payload(canonical_request.as_bytes());

        let string_to_sign = format!(
            "{}\n{}\n{}\n{}",
            algorithm, amz_date, credential_scope, hashed_canonical_request
        );

        string_to_sign
    }

    fn calculate_signature(&self, string_to_sign: &str, date_stamp: &str) -> Result<String, Error> {
        let secret = format!("AWS4{}", self.secret_key);

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
        mac.update(date_stamp.as_bytes());
        let date_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&date_key)?;
        mac.update(self.region.as_bytes());
        let date_region_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&date_region_key)?;
        mac.update(self.service.as_bytes());
        let date_region_service_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&date_region_service_key)?;
        mac.update(b"aws4_request");
        let signing_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&signing_key)?;
        mac.update(string_to_sign.as_bytes());
        let signature = mac.finalize().into_bytes();

        Ok(hex::encode(signature))
    }

    fn hash_payload(&self, payload: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(payload);
        hex::encode(hasher.finalize())
    }
}

pub struct S3Client<HC: HttpClient> {
    http_client: Arc<HC>,
    signer: AwsSignatureV4,
}

impl<HC: HttpClient> S3Client<HC> {
    pub fn new(
        access_key: String,
        secret_key: String,
        region: String,
        http_client: impl Into<Arc<HC>>,
    ) -> Self {
        Self {
            http_client: http_client.into(),
            signer: AwsSignatureV4::for_s3(access_key, secret_key, region),
        }
    }

    pub async fn put_object(
        &self,
        request_id: &str,
        bucket: &str,
        object_name: &str,
        content: Bytes,
    ) -> Result<(), golem_stt::error::Error> {
        let timestamp = Utc::now();
        let uri = format!("https://{}.s3.amazonaws.com/{}", bucket, object_name);

        let request = Request::builder()
            .method("PUT")
            .uri(&uri)
            .body(content)
            .map_err(|e| (request_id.to_string(), client::Error::HttpError(e)))?;

        let signed_request = self
            .signer
            .sign_request(request, timestamp)
            .map_err(|err| {
                (
                    request_id.to_string(),
                    client::Error::Generic(format!("Failed to sign request: {}", err)),
                )
            })?;

        let response = self
            .http_client
            .execute(signed_request)
            .await
            .map_err(|err| (request_id.to_string(), err))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|e| format!("Unknown error, {e}"));

            let status = response.status();
            let request_id = request_id.to_string();

            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!("S3 PutObject bad request: {}", error_body),
                }),
                500..=599 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "S3 PutObject server error ({}): {}",
                        status, error_body
                    ),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!(
                        "S3 PutObject unexpected error ({}): {}",
                        status, error_body
                    ),
                }),
            }
        }
    }

    pub async fn delete_object(
        &self,
        request_id: &str,
        bucket: &str,
        object_name: &str,
    ) -> Result<(), golem_stt::error::Error> {
        let timestamp = Utc::now();
        let uri = format!("https://{}.s3.amazonaws.com/{}", bucket, object_name);

        let request = Request::builder()
            .method("DELETE")
            .uri(&uri)
            .body(Bytes::new())
            .map_err(|e| (request_id.to_string(), client::Error::HttpError(e)))?;

        let signed_request = self
            .signer
            .sign_request(request, timestamp)
            .map_err(|err| {
                (
                    request_id.to_string(),
                    client::Error::Generic(format!("Failed to sign request: {}", err)),
                )
            })?;

        let response = self
            .http_client
            .execute(signed_request)
            .await
            .map_err(|err| (request_id.to_string(), err))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|e| format!("Unknown error, {e}"));

            let status = response.status();
            let request_id = request_id.to_string();

            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!("S3 DeleteObject bad request: {}", error_body),
                }),
                500..=599 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "S3 DeleteObject server error ({}): {}",
                        status, error_body
                    ),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!(
                        "S3 DeleteObject unexpected error ({}): {}",
                        status, error_body
                    ),
                }),
            }
        }
    }
}

// https://docs.aws.amazon.com/transcribe/latest/APIReference/API_CreateVocabulary.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CreateVocabularyRequest {
    pub vocabulary_name: String,
    pub language_code: String,
    pub phrases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary_file_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_access_role_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CreateVocabularyResponse {
    pub vocabulary_name: String,
    pub language_code: String,
    pub vocabulary_state: String,
    pub last_modified_time: f64,
    pub failure_reason: Option<String>,
}

// https://docs.aws.amazon.com/transcribe/latest/APIReference/API_GetVocabulary.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GetVocabularyRequest {
    pub vocabulary_name: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GetVocabularyResponse {
    pub vocabulary_name: String,
    pub language_code: String,
    pub vocabulary_state: String,
    pub last_modified_time: f64,
    pub failure_reason: Option<String>,
    pub download_uri: Option<String>,
}

// https://docs.aws.amazon.com/transcribe/latest/APIReference/API_DeleteVocabulary.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteVocabularyRequest {
    pub vocabulary_name: String,
}

// https://docs.aws.amazon.com/transcribe/latest/APIReference/API_StartTranscriptionJob.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StartTranscriptionJobRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_redaction: Option<ContentRedaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identify_language: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identify_multiple_languages: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_execution_settings: Option<JobExecutionSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_encryption_context: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_id_settings: Option<std::collections::HashMap<String, LanguageIdSettings>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_options: Option<Vec<String>>,
    pub media: Media,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_sample_rate_hertz: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bucket_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_encryption_kms_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitles: Option<Subtitles>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toxicity_detection: Option<Vec<ToxicityDetectionSettings>>,
    pub transcription_job_name: String,
}

// ContentRedaction - https://docs.aws.amazon.com/transcribe/latest/APIReference/API_ContentRedaction.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContentRedaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pii_entity_types: Option<Vec<String>>,
    pub redaction_output: String,
    pub redaction_type: String,
}

// JobExecutionSettings - https://docs.aws.amazon.com/transcribe/latest/APIReference/API_JobExecutionSettings.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct JobExecutionSettings {
    pub allow_deferred_execution: bool,
    pub data_access_role_arn: String,
}

// LanguageIdSettings - https://docs.aws.amazon.com/transcribe/latest/APIReference/API_LanguageIdSettings.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LanguageIdSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary_filter_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Media {
    pub media_file_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redacted_media_file_uri: Option<String>,
}

// ModelSettings - https://docs.aws.amazon.com/transcribe/latest/APIReference/API_ModelSettings.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ModelSettings {
    pub language_model_name: String,
}

// Subtitles - https://docs.aws.amazon.com/transcribe/latest/APIReference/API_Subtitles.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Subtitles {
    pub formats: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_start_index: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_identification: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_alternatives: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_speaker_labels: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_alternatives: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_speaker_labels: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary_filter_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary_filter_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary_name: Option<String>,
}

// ToxicityDetectionSettings - https://docs.aws.amazon.com/transcribe/latest/APIReference/API_ToxicityDetectionSettings.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ToxicityDetectionSettings {
    pub toxicity_categories: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StartTranscriptionJobResponse {
    pub transcription_job: TranscriptionJob,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct TranscriptionJob {
    pub transcription_job_name: String,
    pub transcription_job_status: String,
    pub language_code: Option<String>,
    pub media: Option<Media>,
    pub media_format: Option<String>,
    pub media_sample_rate_hertz: Option<i32>,
    pub creation_time: Option<f64>,
    pub completion_time: Option<f64>,
    pub start_time: Option<f64>,
    pub failure_reason: Option<String>,
    pub settings: Option<Settings>,
    pub transcript: Option<Transcript>,
    pub tags: Option<Vec<Tag>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Transcript {
    pub transcript_file_uri: Option<String>,
    pub redacted_transcript_file_uri: Option<String>,
}

// https://docs.aws.amazon.com/transcribe/latest/APIReference/API_GetTranscriptionJob.html
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GetTranscriptionJobRequest {
    pub transcription_job_name: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GetTranscriptionJobResponse {
    pub transcription_job: TranscriptionJob,
}

pub struct TranscribeClient<HC: golem_stt::client::HttpClient> {
    http_client: std::sync::Arc<HC>,
    signer: AwsSignatureV4,
}

impl<HC: golem_stt::client::HttpClient> TranscribeClient<HC> {
    pub fn new(
        access_key: String,
        secret_key: String,
        region: String,
        http_client: impl Into<std::sync::Arc<HC>>,
    ) -> Self {
        Self {
            http_client: http_client.into(),
            signer: AwsSignatureV4::for_transcribe(access_key, secret_key, region),
        }
    }

    pub async fn create_vocabulary(
        &self,
        vocabulary_name: String,
        language_code: String,
        phrases: Vec<String>,
    ) -> Result<CreateVocabularyResponse, golem_stt::error::Error> {
        let timestamp = Utc::now();
        let uri = format!(
            "https://transcribe.{}.amazonaws.com/",
            self.signer.get_region()
        );

        let request_body = CreateVocabularyRequest {
            vocabulary_name: vocabulary_name.clone(),
            language_code,
            phrases,
            vocabulary_file_uri: None,
            data_access_role_arn: None,
            tags: None,
        };

        let json_body = serde_json::to_string(&request_body).map_err(|e| {
            (
                vocabulary_name.clone(),
                client::Error::Generic(format!("Failed to serialize request: {}", e)),
            )
        })?;

        let request = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("Content-Type", "application/x-amz-json-1.1")
            .header(
                "X-Amz-Target",
                "com.amazonaws.transcribe.Transcribe.CreateVocabulary",
            )
            .body(Bytes::from(json_body))
            .map_err(|e| (vocabulary_name.clone(), client::Error::HttpError(e)))?;

        let signed_request = self
            .signer
            .sign_request(request, timestamp)
            .map_err(|err| {
                (
                    vocabulary_name.clone(),
                    client::Error::Generic(format!("Failed to sign request: {}", err)),
                )
            })?;

        let response = self
            .http_client
            .execute(signed_request)
            .await
            .map_err(|err| (vocabulary_name.clone(), err))?;

        if response.status().is_success() {
            let vocabulary_response: CreateVocabularyResponse =
                serde_json::from_slice(response.body()).map_err(|e| {
                    (
                        vocabulary_name.clone(),
                        client::Error::Generic(format!("Failed to deserialize response: {}", e)),
                    )
                })?;

            Ok(vocabulary_response)
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|_| "Unknown error".to_string());

            let status = response.status();
            let request_id = vocabulary_name.clone();

            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!(
                        "Transcribe CreateVocabulary bad request: {}",
                        error_body
                    ),
                }),
                403 => Err(golem_stt::error::Error::APIForbidden {
                    request_id,
                    provider_error: format!(
                        "Transcribe CreateVocabulary forbidden: {}",
                        error_body
                    ),
                }),
                500 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe CreateVocabulary server error: {}",
                        error_body
                    ),
                }),
                503 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe CreateVocabulary service unavailable: {}",
                        error_body
                    ),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!(
                        "Transcribe CreateVocabulary unknown error ({}): {}",
                        status, error_body
                    ),
                }),
            }
        }
    }

    pub async fn get_vocabulary(
        &self,
        vocabulary_name: String,
    ) -> Result<GetVocabularyResponse, golem_stt::error::Error> {
        let timestamp = Utc::now();
        let uri = format!(
            "https://transcribe.{}.amazonaws.com/",
            self.signer.get_region()
        );

        let request_body = GetVocabularyRequest {
            vocabulary_name: vocabulary_name.clone(),
        };

        let json_body = serde_json::to_string(&request_body).map_err(|e| {
            (
                vocabulary_name.clone(),
                client::Error::Generic(format!("Failed to serialize request: {}", e)),
            )
        })?;

        let request = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("Content-Type", "application/x-amz-json-1.1")
            .header(
                "X-Amz-Target",
                "com.amazonaws.transcribe.Transcribe.GetVocabulary",
            )
            .body(Bytes::from(json_body))
            .map_err(|e| (vocabulary_name.clone(), client::Error::HttpError(e)))?;

        let signed_request = self
            .signer
            .sign_request(request, timestamp)
            .map_err(|err| {
                (
                    vocabulary_name.clone(),
                    client::Error::Generic(format!("Failed to sign request: {}", err)),
                )
            })?;

        let response = self
            .http_client
            .execute(signed_request)
            .await
            .map_err(|err| (vocabulary_name.clone(), err))?;

        if response.status().is_success() {
            let vocabulary_response: GetVocabularyResponse =
                serde_json::from_slice(response.body()).map_err(|e| {
                    (
                        vocabulary_name.clone(),
                        client::Error::Generic(format!("Failed to deserialize response: {}", e)),
                    )
                })?;

            Ok(vocabulary_response)
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|_| "Unknown error".to_string());

            let status = response.status();
            let request_id = vocabulary_name.clone();

            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!("Transcribe GetVocabulary bad request: {}", error_body),
                }),
                403 => Err(golem_stt::error::Error::APIForbidden {
                    request_id,
                    provider_error: format!("Transcribe GetVocabulary forbidden: {}", error_body),
                }),
                500 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetVocabulary server error: {}",
                        error_body
                    ),
                }),
                503 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetVocabulary service unavailable: {}",
                        error_body
                    ),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetVocabulary unknown error ({}): {}",
                        status, error_body
                    ),
                }),
            }
        }
    }

    pub async fn wait_for_vocabulary_ready<RT: AsyncRuntime>(
        &self,
        runtime: &RT,
        request_id: &str,
        max_wait_time: Duration,
    ) -> Result<(), golem_stt::error::Error> {
        let start_time = std::time::Instant::now();
        let mut retry_delay = Duration::from_millis(500);
        let max_delay = Duration::from_secs(30);

        loop {
            if start_time.elapsed() > max_wait_time {
                return Err(golem_stt::error::Error::APIBadRequest {
                    request_id: request_id.to_string(),
                    provider_error: "Vocabulary creation timed out".to_string(),
                });
            }

            runtime.sleep(retry_delay).await;

            let res = self.get_vocabulary(request_id.to_string()).await?;

            match res.vocabulary_state.as_str() {
                "READY" => return Ok(()),
                "FAILED" => {
                    return Err(golem_stt::error::Error::APIBadRequest {
                        request_id: request_id.to_string(),
                        provider_error: format!(
                            "Vocabulary creation failed: {}",
                            res.failure_reason
                                .unwrap_or_else(|| "Unknown error".to_string())
                        ),
                    });
                }
                "PENDING" => {
                    retry_delay = std::cmp::min(
                        Duration::from_millis((retry_delay.as_millis() * 2) as u64),
                        max_delay,
                    );
                }
                other => {
                    return Err(golem_stt::error::Error::APIBadRequest {
                        request_id: request_id.to_string(),
                        provider_error: format!("Unexpected vocabulary state: {}", other),
                    });
                }
            }
        }
    }

    pub async fn delete_vocabulary(
        &self,
        vocabulary_name: &str,
    ) -> Result<(), golem_stt::error::Error> {
        let timestamp = Utc::now();
        let uri = format!(
            "https://transcribe.{}.amazonaws.com/",
            self.signer.get_region()
        );

        let request_body = DeleteVocabularyRequest {
            vocabulary_name: vocabulary_name.to_string(),
        };

        let json_body = serde_json::to_string(&request_body).map_err(|e| {
            (
                vocabulary_name.to_string(),
                client::Error::Generic(format!("Failed to serialize request: {}", e)),
            )
        })?;

        let request = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("Content-Type", "application/x-amz-json-1.1")
            .header(
                "X-Amz-Target",
                "com.amazonaws.transcribe.Transcribe.DeleteVocabulary",
            )
            .body(Bytes::from(json_body))
            .map_err(|e| (vocabulary_name.to_string(), client::Error::HttpError(e)))?;

        let signed_request = self
            .signer
            .sign_request(request, timestamp)
            .map_err(|err| {
                (
                    vocabulary_name.to_string(),
                    client::Error::Generic(format!("Failed to sign request: {}", err)),
                )
            })?;

        let response = self
            .http_client
            .execute(signed_request)
            .await
            .map_err(|err| (vocabulary_name.to_string(), err))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|_| "Unknown error".to_string());

            let status = response.status();
            let request_id = vocabulary_name.to_string();

            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!(
                        "Transcribe DeleteVocabulary bad request: {}",
                        error_body
                    ),
                }),
                403 => Err(golem_stt::error::Error::APIForbidden {
                    request_id,
                    provider_error: format!(
                        "Transcribe DeleteVocabulary forbidden: {}",
                        error_body
                    ),
                }),
                500 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe DeleteVocabulary server error: {}",
                        error_body
                    ),
                }),
                503 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe DeleteVocabulary service unavailable: {}",
                        error_body
                    ),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!(
                        "Transcribe DeleteVocabulary unknown error ({}): {}",
                        status, error_body
                    ),
                }),
            }
        }
    }

    pub async fn start_transcription_job(
        &self,
        transcription_job_name: String,
        media_file_uri: String,
        audio_config: AudioConfig,
        transcription_config: Option<TranscriptionConfig>,
        vocabulary_name: Option<String>,
    ) -> Result<StartTranscriptionJobResponse, golem_stt::error::Error> {
        let timestamp = Utc::now();
        let uri = format!(
            "https://transcribe.{}.amazonaws.com/",
            self.signer.get_region()
        );

        let language_code = transcription_config
            .as_ref()
            .and_then(|config| config.language.as_ref().map(|lang| lang.to_string()));
        let use_identify_language = language_code.is_none();
        let enable_channel_identification = audio_config.channels.filter(|ch| *ch > 1).is_some();
        let enable_speaker_diarization = transcription_config
            .as_ref()
            .map(|config| config.enable_speaker_diarization)
            .unwrap_or(false);

        let model_settings = if !use_identify_language {
            transcription_config
                .as_ref()
                .and_then(|config| config.model.as_deref())
                .map(|model| ModelSettings {
                    language_model_name: model.to_string(),
                })
        } else {
            None
        };

        let settings = if enable_channel_identification
            || enable_speaker_diarization
            || (!use_identify_language && vocabulary_name.is_some())
        {
            Some(Settings {
                channel_identification: if enable_channel_identification {
                    Some(true)
                } else {
                    None
                },
                max_alternatives: None,
                max_speaker_labels: if enable_speaker_diarization {
                    Some(30)
                } else {
                    None
                },
                show_alternatives: None,
                show_speaker_labels: if enable_speaker_diarization {
                    Some(true)
                } else {
                    None
                },
                vocabulary_filter_method: None,
                vocabulary_filter_name: None,
                vocabulary_name: if !use_identify_language {
                    vocabulary_name
                } else {
                    None
                },
            })
        } else {
            None
        };

        let request_body = StartTranscriptionJobRequest {
            content_redaction: None,
            identify_language: if use_identify_language {
                Some(true)
            } else {
                None
            },
            identify_multiple_languages: None,
            job_execution_settings: None,
            kms_encryption_context: None,
            language_code,
            language_id_settings: None,
            language_options: None,
            media: Media {
                media_file_uri,
                redacted_media_file_uri: None,
            },
            media_format: Some(audio_config.format.to_string()),
            media_sample_rate_hertz: None,
            model_settings,
            output_bucket_name: None,
            output_encryption_kms_key_id: None,
            output_key: None,
            settings,
            subtitles: None,
            tags: None,
            toxicity_detection: None,
            transcription_job_name: transcription_job_name.clone(),
        };

        let json_body = serde_json::to_string(&request_body).map_err(|e| {
            (
                transcription_job_name.clone(),
                client::Error::Generic(format!("Failed to serialize request: {}", e)),
            )
        })?;

        let request = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("Content-Type", "application/x-amz-json-1.1")
            .header(
                "X-Amz-Target",
                "com.amazonaws.transcribe.Transcribe.StartTranscriptionJob",
            )
            .body(Bytes::from(json_body))
            .map_err(|e| (transcription_job_name.clone(), client::Error::HttpError(e)))?;

        let signed_request = self
            .signer
            .sign_request(request, timestamp)
            .map_err(|err| {
                (
                    transcription_job_name.clone(),
                    client::Error::Generic(format!("Failed to sign request: {}", err)),
                )
            })?;

        let response = self
            .http_client
            .execute(signed_request)
            .await
            .map_err(|err| (transcription_job_name.clone(), err))?;

        if response.status().is_success() {
            let transcription_response: StartTranscriptionJobResponse =
                serde_json::from_slice(response.body()).map_err(|e| {
                    (
                        transcription_job_name.clone(),
                        client::Error::Generic(format!("Failed to deserialize response: {}", e)),
                    )
                })?;

            Ok(transcription_response)
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|_| "Unknown error".to_string());

            let status = response.status();
            let request_id = transcription_job_name.clone();

            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!(
                        "Transcribe StartTranscriptionJob bad request: {}",
                        error_body
                    ),
                }),
                403 => Err(golem_stt::error::Error::APIForbidden {
                    request_id,
                    provider_error: format!(
                        "Transcribe StartTranscriptionJob forbidden: {}",
                        error_body
                    ),
                }),
                500 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe StartTranscriptionJob server error: {}",
                        error_body
                    ),
                }),
                503 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe StartTranscriptionJob service unavailable: {}",
                        error_body
                    ),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!(
                        "Transcribe StartTranscriptionJob unknown error ({}): {}",
                        status, error_body
                    ),
                }),
            }
        }
    }

    pub async fn get_transcription_job(
        &self,
        transcription_job_name: String,
    ) -> Result<GetTranscriptionJobResponse, golem_stt::error::Error> {
        let timestamp = Utc::now();
        let uri = format!(
            "https://transcribe.{}.amazonaws.com/",
            self.signer.get_region()
        );

        let request_body = GetTranscriptionJobRequest {
            transcription_job_name: transcription_job_name.clone(),
        };

        let json_body = serde_json::to_string(&request_body).map_err(|e| {
            (
                transcription_job_name.clone(),
                client::Error::Generic(format!("Failed to serialize request: {}", e)),
            )
        })?;

        let request = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("Content-Type", "application/x-amz-json-1.1")
            .header(
                "X-Amz-Target",
                "com.amazonaws.transcribe.Transcribe.GetTranscriptionJob",
            )
            .body(Bytes::from(json_body))
            .map_err(|e| (transcription_job_name.clone(), client::Error::HttpError(e)))?;

        let signed_request = self
            .signer
            .sign_request(request, timestamp)
            .map_err(|err| {
                (
                    transcription_job_name.clone(),
                    client::Error::Generic(format!("Failed to sign request: {}", err)),
                )
            })?;

        let response = self
            .http_client
            .execute(signed_request)
            .await
            .map_err(|err| (transcription_job_name.clone(), err))?;

        if response.status().is_success() {
            let transcription_response: GetTranscriptionJobResponse =
                serde_json::from_slice(response.body()).map_err(|e| {
                    (
                        transcription_job_name.clone(),
                        client::Error::Generic(format!("Failed to deserialize response: {}", e)),
                    )
                })?;

            Ok(transcription_response)
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|_| "Unknown error".to_string());

            let status = response.status();
            let request_id = transcription_job_name.clone();

            // Map HTTP status codes based on AWS Transcribe GetTranscriptionJob API spec
            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetTranscriptionJob bad request: {}",
                        error_body
                    ),
                }),
                403 => Err(golem_stt::error::Error::APIForbidden {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetTranscriptionJob forbidden: {}",
                        error_body
                    ),
                }),
                500 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetTranscriptionJob server error: {}",
                        error_body
                    ),
                }),
                503 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetTranscriptionJob service unavailable: {}",
                        error_body
                    ),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!(
                        "Transcribe GetTranscriptionJob unknown error ({}): {}",
                        status, error_body
                    ),
                }),
            }
        }
    }

    pub async fn wait_for_transcription_job_completion<RT: AsyncRuntime>(
        &self,
        runtime: &RT,
        transcription_job_name: &str,
        max_wait_time: Duration,
    ) -> Result<GetTranscriptionJobResponse, golem_stt::error::Error> {
        let start_time = std::time::Instant::now();
        let mut retry_delay = Duration::from_millis(2000); // Start with 2 seconds for transcription jobs
        let max_delay = Duration::from_secs(60); // Max 60 seconds between retries

        loop {
            if start_time.elapsed() > max_wait_time {
                return Err(golem_stt::error::Error::APIBadRequest {
                    request_id: transcription_job_name.to_string(),
                    provider_error: "Transcription job timed out".to_string(),
                });
            }

            runtime.sleep(retry_delay).await;

            let res = self
                .get_transcription_job(transcription_job_name.to_string())
                .await?;

            match res.transcription_job.transcription_job_status.as_str() {
                "COMPLETED" => {
                    return Ok(res);
                }
                "FAILED" => {
                    return Err(golem_stt::error::Error::APIBadRequest {
                        request_id: transcription_job_name.to_string(),
                        provider_error: format!(
                            "Transcription job failed: {}",
                            res.transcription_job
                                .failure_reason
                                .as_ref()
                                .unwrap_or(&"Unknown error".to_string())
                        ),
                    });
                }
                "IN_PROGRESS" | "QUEUED" => {
                    // Continue polling with exponential backoff
                    retry_delay = std::cmp::min(
                        Duration::from_millis((retry_delay.as_millis() as f64 * 1.5) as u64),
                        max_delay,
                    );
                }
                other => {
                    return Err(golem_stt::error::Error::APIBadRequest {
                        request_id: transcription_job_name.to_string(),
                        provider_error: format!("Unexpected transcription job status: {}", other),
                    });
                }
            }
        }
    }

    pub async fn download_transcript_json(
        &self,
        transcription_job_name: &str,
        transcript_uri: &str,
    ) -> Result<TranscribeOutput, golem_stt::error::Error> {
        let request = http::Request::builder()
            .method("GET")
            .uri(transcript_uri)
            .header("Accept", "application/json")
            .body(bytes::Bytes::new())
            .map_err(|e| {
                (
                    transcription_job_name.to_string(),
                    client::Error::HttpError(e),
                )
            })?;

        let response = self
            .http_client
            .execute(request)
            .await
            .map_err(|err| (transcription_job_name.to_string(), err))?;

        if response.status().is_success() {
            let transcript_json: crate::client::TranscribeOutput =
                serde_json::from_slice(response.body()).map_err(|e| {
                    golem_stt::error::Error::Client(
                        transcription_job_name.to_string(),
                        client::Error::Generic(format!("Failed to deserialize response: {}", e)),
                    )
                })?;

            Ok(transcript_json)
        } else {
            let error_body = String::from_utf8(response.body().to_vec())
                .unwrap_or_else(|_| "Unknown error".to_string());

            let status = response.status();
            let request_id = transcription_job_name.to_string();

            // Map HTTP status codes based on S3 GET object behavior for transcript download
            match status.as_u16() {
                400 => Err(golem_stt::error::Error::APIBadRequest {
                    request_id,
                    provider_error: format!("Transcript download bad request: {}", error_body),
                }),
                403 => Err(golem_stt::error::Error::APIForbidden {
                    request_id,
                    provider_error: format!("Transcript download forbidden (expired URL or insufficient permissions): {}", error_body),
                }),
                404 => Err(golem_stt::error::Error::APINotFound {
                    request_id,
                    provider_error: format!("Transcript file not found: {}", error_body),
                }),
                500..=599 => Err(golem_stt::error::Error::APIInternalServerError {
                    request_id,
                    provider_error: format!("Transcript download server error ({}): {}", status, error_body),
                }),
                _ => Err(golem_stt::error::Error::APIUnknown {
                    request_id,
                    provider_error: format!("Transcript download unknown error ({}): {}", status, error_body),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Ref, RefCell},
        collections::VecDeque,
    };

    use super::*;
    use aws_credential_types::Credentials;
    use http::{Method, Request, Response, StatusCode};

    use aws_sigv4::{
        http_request::{sign, SignableBody, SignableRequest, SigningSettings},
        sign::v4,
    };
    use wasi_async_runtime::block_on;

    fn sign_with_aws_sdk(
        mut request: Request<Bytes>,
        access_key: &str,
        secret_key: &str,
        region: &str,
        service: &str,
        timestamp: DateTime<Utc>,
    ) -> Request<Bytes> {
        let creds = Credentials::new(access_key, secret_key, None, None, "iam");
        let identity = creds.into();

        let signing_settings = SigningSettings::default();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(region)
            .name(service)
            .time(timestamp.into())
            .settings(signing_settings)
            .build()
            .unwrap()
            .into();

        let mut hasher = Sha256::new();
        hasher.update(request.body().as_ref());
        let hashed_content = hex::encode(hasher.finalize());

        request.headers_mut().append(
            "x-amz-content-sha256",
            HeaderValue::from_str(&hashed_content).unwrap(),
        );

        let signable_request = SignableRequest::new(
            request.method().as_str(),
            request.uri().to_string(),
            request
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str(), std::str::from_utf8(v.as_bytes()).unwrap())),
            SignableBody::Bytes(request.body().as_ref()),
        )
        .unwrap();

        let (signing_instructions, _signature) = sign(signable_request, &signing_params)
            .unwrap()
            .into_parts();
        signing_instructions.apply_to_request_http1x(&mut request);

        request
    }

    // test constructd based on spec here https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
    #[test]
    fn test_uri_encoding_all_characters() {
        let signer = AwsSignatureV4::for_s3(
            "test".to_string(),
            "test".to_string(),
            "us-east-1".to_string(),
        );

        assert_eq!(signer.canonical_uri("test file.txt"), "test%20file.txt");

        assert_eq!(signer.canonical_uri("test!file.txt"), "test%21file.txt");

        assert_eq!(signer.canonical_uri("test\"file.txt"), "test%22file.txt");

        assert_eq!(signer.canonical_uri("test#file.txt"), "test%23file.txt");

        assert_eq!(signer.canonical_uri("test$file.txt"), "test%24file.txt");

        assert_eq!(signer.canonical_uri("test%file.txt"), "test%25file.txt");

        assert_eq!(signer.canonical_uri("test'file.txt"), "test%27file.txt");

        assert_eq!(signer.canonical_uri("test(file.txt"), "test%28file.txt");

        assert_eq!(signer.canonical_uri("test)file.txt"), "test%29file.txt");

        assert_eq!(signer.canonical_uri("test*file.txt"), "test%2Afile.txt");

        assert_eq!(signer.canonical_uri("test,file.txt"), "test%2Cfile.txt");

        assert_eq!(signer.canonical_uri("folder/file.txt"), "folder/file.txt");

        assert_eq!(signer.canonical_uri("test:file.txt"), "test%3Afile.txt");

        assert_eq!(signer.canonical_uri("test;file.txt"), "test%3Bfile.txt");

        assert_eq!(signer.canonical_uri("test?file.txt"), "test%3Ffile.txt");

        assert_eq!(signer.canonical_uri("test@file.txt"), "test%40file.txt");

        assert_eq!(signer.canonical_uri("test[file.txt"), "test%5Bfile.txt");

        assert_eq!(signer.canonical_uri("test\\file.txt"), "test%5Cfile.txt");

        assert_eq!(signer.canonical_uri("test]file.txt"), "test%5Dfile.txt");

        assert_eq!(signer.canonical_uri("test^file.txt"), "test%5Efile.txt");

        assert_eq!(signer.canonical_uri("test`file.txt"), "test%60file.txt");

        assert_eq!(signer.canonical_uri("test{file.txt"), "test%7Bfile.txt");

        assert_eq!(signer.canonical_uri("test|file.txt"), "test%7Cfile.txt");

        assert_eq!(signer.canonical_uri("test}file.txt"), "test%7Dfile.txt");

        assert_eq!(signer.canonical_uri("test~file.txt"), "test~file.txt");

        assert_eq!(
            signer.canonical_uri("test-file_123.txt"),
            "test-file_123.txt"
        );
    }

    #[test]
    fn test_query_encoding_all_characters() {
        let signer = AwsSignatureV4::for_s3(
            "test".to_string(),
            "test".to_string(),
            "us-east-1".to_string(),
        );

        assert_eq!(
            signer.canonical_query_string("key=value=with=equals"),
            "key=value%3Dwith%3Dequals",
        );

        assert_eq!(
            signer.canonical_query_string("key=value+with+plus"),
            "key=value%2Bwith%2Bplus",
        );

        assert_eq!(
            signer.canonical_query_string("key=value with spaces"),
            "key=value%20with%20spaces",
        );

        assert_eq!(
            signer.canonical_query_string("filter=name=\"John Doe\"&sort=date:desc"),
            "filter=name%3D%22John%20Doe%22&sort=date%3Adesc",
        );

        assert_eq!(
            signer.canonical_query_string("z-param=last&a-param=first&m-param=middle"),
            "a-param=first&m-param=middle&z-param=last",
        );
    }

    #[test]
    fn test_s3_get_object_authorization_header() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_s3(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        let request = Request::builder()
            .method(Method::GET)
            .uri("s3://examplebucket.s3.amazonaws.com/foo/bar/test@file.txt")
            .header("Range", "bytes=0-9")
            .body(vec![].into())
            .unwrap();

        let request_for_aws_sdk = request.clone();

        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        let aws_signed_request = sign_with_aws_sdk(
            request_for_aws_sdk,
            access_key,
            secret_key,
            region,
            "s3",
            timestamp,
        );

        let expected_auth = aws_signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }

    #[test]
    fn test_s3_put_object_authorization_header() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_s3(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        // Create the exact request from your specification
        let request = Request::builder()
            .method(Method::PUT)
            .uri("s3://examplebucket.s3.amazonaws.com/test$file.text")
            .header("Date", "Fri, 24 May 2013 00:00:00 GMT")
            .header("x-amz-storage-class", "REDUCED_REDUNDANCY")
            .body(b"Welcome to Amazon S3.".to_vec().into())
            .unwrap();

        let request_for_aws_sdk = request.clone();

        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        let aws_signed_request = sign_with_aws_sdk(
            request_for_aws_sdk,
            access_key,
            secret_key,
            region,
            "s3",
            timestamp,
        );

        let expected_auth = aws_signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }

    #[test]
    fn test_s3_list_objects_authorization_header() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_s3(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        let request = Request::builder()
            .method(Method::GET)
            .uri("s3://examplebucket.s3.amazonaws.com/?max-keys=2&prefix=J")
            .body(vec![].into())
            .unwrap();

        let request_for_aws_sdk = request.clone();

        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        let aws_signed_request = sign_with_aws_sdk(
            request_for_aws_sdk,
            access_key,
            secret_key,
            region,
            "s3",
            timestamp,
        );

        let expected_auth = aws_signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }

    #[test]
    fn test_batch_transcription_authorization_header() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_transcribe(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        let body = r#"
            {
                "TranscriptionJobName": "my-first-transcription-job",
                "LanguageCode": "en-US",
                "Media": {
                    "MediaFileUri": "s3://amzn-s3-demo-bucket/my-input-files/my-media-file.flac"
                },
                "OutputBucketName": "amzn-s3-demo-bucket",
                "OutputKey": "my-output-files/"
            }
            "#;

        // Create the exact request from your specification
        let request = Request::builder()
            .method(Method::GET)
            .uri("s3://examplebucket.s3.amazonaws.com/?max-keys=2&prefix=J")
            .body(body.as_bytes().into())
            .unwrap();

        let request_for_aws_sdk = request.clone();

        // Parse the Date header directly: "Fri, 24 May 2013 00:00:00 GMT"
        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        // Get the authorization header
        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        let aws_signed_request = sign_with_aws_sdk(
            request_for_aws_sdk,
            access_key,
            secret_key,
            region,
            "transcribe",
            timestamp,
        );

        let expected_auth = aws_signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }

    struct MockHttpClient {
        pub responses: RefCell<VecDeque<Result<Response<Bytes>, client::Error>>>,
        pub captured_requests: RefCell<Vec<Request<Bytes>>>,
    }

    #[allow(unused)]
    impl MockHttpClient {
        pub fn new() -> Self {
            Self {
                responses: RefCell::new(VecDeque::new()),
                captured_requests: RefCell::new(Vec::new()),
            }
        }

        pub fn expect_response(&self, response: Response<Bytes>) {
            self.responses.borrow_mut().push_back(Ok(response));
        }

        pub fn get_captured_requests(&self) -> Ref<Vec<Request<Bytes>>> {
            self.captured_requests.borrow()
        }

        pub fn clear_captured_requests(&self) {
            self.captured_requests.borrow_mut().clear();
        }

        pub fn captured_request_count(&self) -> usize {
            self.captured_requests.borrow().len()
        }

        pub fn last_captured_request(&self) -> Option<Ref<Request<Bytes>>> {
            let borrow = self.captured_requests.borrow();
            if borrow.is_empty() {
                None
            } else {
                Some(Ref::map(borrow, |requests| requests.last().unwrap()))
            }
        }
    }

    impl HttpClient for MockHttpClient {
        async fn execute(&self, request: Request<Bytes>) -> Result<Response<Bytes>, client::Error> {
            self.captured_requests.borrow_mut().push(request);
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(client::Error::Generic("unexpected error".to_string())))
        }
    }

    #[test]
    fn test_s3_put_object_request() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        mock_client.expect_response(
            Response::builder()
                .status(StatusCode::OK)
                .body(Bytes::new())
                .unwrap(),
        );

        let s3_client: S3Client<MockHttpClient> = S3Client::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let bucket = "test-bucket";
        let object_name = "test-object.txt";
        let content = Bytes::from("Hello, World!");

        let _result = block_on(|_| async {
            s3_client
                .put_object("some-request-id", bucket, object_name, content.clone())
                .await
        });

        let captured_request = mock_client.last_captured_request();
        let request = captured_request.as_ref().unwrap();

        assert_eq!(request.method(), "PUT");

        let expected_uri = format!("https://{}.s3.amazonaws.com/{}", bucket, object_name);
        assert_eq!(request.uri().to_string(), expected_uri);

        assert_eq!(request.body(), &content);

        assert!(request.headers().contains_key("x-amz-date"));
        assert!(request.headers().contains_key("x-amz-content-sha256"));
        assert!(request.headers().contains_key("authorization"));
        assert!(request.headers().contains_key("host"));

        let auth_header = request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth_header.starts_with("AWS4-HMAC-SHA256"));
        assert!(auth_header.contains("Credential="));
        assert!(auth_header.contains("SignedHeaders="));
        assert!(auth_header.contains("Signature="));

        let host_header = request.headers().get("host").unwrap().to_str().unwrap();
        assert_eq!(host_header, format!("{}.s3.amazonaws.com", bucket));
    }

    #[test]
    fn test_s3_delete_object_request() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        mock_client.expect_response(
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(Bytes::new())
                .unwrap(),
        );

        let s3_client: S3Client<MockHttpClient> = S3Client::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let bucket = "test-bucket";
        let object_name = "test-object.txt";

        let result = block_on(|_| async {
            s3_client
                .delete_object("some-request-id", bucket, object_name)
                .await
        });

        assert!(result.is_ok());

        let captured_request = mock_client.last_captured_request();
        let request = captured_request.as_ref().unwrap();

        assert_eq!(request.method(), "DELETE");

        let expected_uri = format!("https://{}.s3.amazonaws.com/{}", bucket, object_name);
        assert_eq!(request.uri().to_string(), expected_uri);

        assert_eq!(request.body(), &Bytes::new());

        assert!(request.headers().contains_key("x-amz-date"));
        assert!(request.headers().contains_key("x-amz-content-sha256"));
        assert!(request.headers().contains_key("authorization"));
        assert!(request.headers().contains_key("host"));

        let auth_header = request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth_header.starts_with("AWS4-HMAC-SHA256"));
        assert!(auth_header.contains("Credential="));
        assert!(auth_header.contains("SignedHeaders="));
        assert!(auth_header.contains("Signature="));

        let host_header = request.headers().get("host").unwrap().to_str().unwrap();
        assert_eq!(host_header, format!("{}.s3.amazonaws.com", bucket));
    }

    #[test]
    fn test_transcribe_create_vocabulary_request() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let expected_response = CreateVocabularyResponse {
            vocabulary_name: "test-vocabulary".to_string(),
            language_code: "en-US".to_string(),
            vocabulary_state: "PENDING".to_string(),
            last_modified_time: 1234567890.0,
            failure_reason: None,
        };

        let mock_client = Arc::new(MockHttpClient::new());

        let response_json_str = serde_json::to_string(&expected_response).unwrap();

        let body_bytes = Bytes::from(response_json_str.as_bytes().to_vec());

        mock_client.expect_response(
            Response::builder()
                .status(StatusCode::OK)
                .body(body_bytes)
                .unwrap(),
        );

        let transcribe_client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let vocabulary_name = "test-vocabulary".to_string();
        let language_code = "en-US".to_string();
        let phrases = vec![
            "hello world".to_string(),
            "machine learning".to_string(),
            "artificial intelligence".to_string(),
        ];

        let result = block_on(|_| async {
            transcribe_client
                .create_vocabulary(
                    vocabulary_name.clone(),
                    language_code.clone(),
                    phrases.clone(),
                )
                .await
        });

        assert!(result.is_ok());
        let actual_response = result.unwrap();
        assert_eq!(actual_response, expected_response);

        let request = mock_client.last_captured_request().unwrap();
        assert_eq!(request.method(), "POST");
        assert_eq!(
            request.uri().to_string(),
            format!("https://transcribe.{}.amazonaws.com/", region)
        );
        assert_eq!(
            request.headers().get("content-type").unwrap(),
            "application/x-amz-json-1.1"
        );
        assert_eq!(
            request.headers().get("x-amz-target").unwrap(),
            "com.amazonaws.transcribe.Transcribe.CreateVocabulary"
        );

        let expected_request = CreateVocabularyRequest {
            vocabulary_name,
            language_code,
            phrases,
            vocabulary_file_uri: None,
            data_access_role_arn: None,
            tags: None,
        };

        let actual_request: CreateVocabularyRequest =
            serde_json::from_slice(request.body()).unwrap();
        assert_eq!(actual_request, expected_request);

        assert!(request.headers().contains_key("x-amz-date"));
        assert!(request.headers().contains_key("x-amz-content-sha256"));
        assert!(request.headers().contains_key("authorization"));
        assert!(request.headers().contains_key("host"));

        let auth_header = request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth_header.starts_with("AWS4-HMAC-SHA256"));
        assert!(auth_header.contains("Credential="));
        assert!(auth_header.contains("SignedHeaders="));
        assert!(auth_header.contains("Signature="));
    }

    #[test]
    fn test_transcribe_delete_vocabulary_request() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());
        mock_client.expect_response(
            Response::builder()
                .status(StatusCode::OK)
                .body(Bytes::new())
                .unwrap(),
        );

        let transcribe_client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let vocabulary_name = "test-vocabulary".to_string();
        let result =
            block_on(|_| async { transcribe_client.delete_vocabulary(&vocabulary_name).await });

        assert!(result.is_ok());

        let request = mock_client.last_captured_request().unwrap();
        assert_eq!(request.method(), "POST");
        assert_eq!(
            request.uri().to_string(),
            format!("https://transcribe.{}.amazonaws.com/", region)
        );
        assert_eq!(
            request.headers().get("content-type").unwrap(),
            "application/x-amz-json-1.1"
        );
        assert_eq!(
            request.headers().get("x-amz-target").unwrap(),
            "com.amazonaws.transcribe.Transcribe.DeleteVocabulary"
        );

        let expected_request = DeleteVocabularyRequest { vocabulary_name };

        let actual_request: DeleteVocabularyRequest =
            serde_json::from_slice(request.body()).unwrap();
        assert_eq!(actual_request, expected_request);

        assert!(request.headers().contains_key("x-amz-date"));
        assert!(request.headers().contains_key("x-amz-content-sha256"));
        assert!(request.headers().contains_key("authorization"));
        assert!(request.headers().contains_key("host"));

        let auth_header = request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth_header.starts_with("AWS4-HMAC-SHA256"));
        assert!(auth_header.contains("Credential="));
        assert!(auth_header.contains("SignedHeaders="));
        assert!(auth_header.contains("Signature="));
    }

    #[test]
    fn test_start_transcription_job_basic_request() {
        use crate::client::{AudioConfig, AudioFormat};

        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        let mock_response = r#"{
               "TranscriptionJob": {
                   "TranscriptionJobName": "test-job-basic",
                   "TranscriptionJobStatus": "COMPLETED"
               }
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(mock_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        // Test basic audio config with no transcription config (should use identify_language)
        let audio_config = AudioConfig {
            format: AudioFormat::wav,
            channels: Some(1),
        };

        let _result = block_on(|_| async {
            client
                .start_transcription_job(
                    "test-job-basic".to_string(),
                    "s3://bucket/audio.wav".to_string(),
                    audio_config,
                    None, // No transcription config
                    None, // No vocabulary
                )
                .await
        });

        // Verify the request was constructed correctly
        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartTranscriptionJobRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartTranscriptionJobRequest {
            content_redaction: None,
            identify_language: Some(true), // Should be true when no language specified
            identify_multiple_languages: None,
            job_execution_settings: None,
            kms_encryption_context: None,
            language_code: None, // Should be None when using identify_language
            language_id_settings: None,
            language_options: None,
            media: Media {
                media_file_uri: "s3://bucket/audio.wav".to_string(),
                redacted_media_file_uri: None,
            },
            media_format: Some("wav".to_string()),
            media_sample_rate_hertz: None,
            model_settings: None, // Should be None when using identify_language
            output_bucket_name: None,
            output_encryption_kms_key_id: None,
            output_key: None,
            settings: None, // Should be None for basic single-channel audio
            subtitles: None,
            tags: None,
            toxicity_detection: None,
            transcription_job_name: "test-job-basic".to_string(),
        };

        assert_eq!(
            actual_request, expected_request,
            "Basic request should match expected structure"
        );
    }

    #[test]
    fn test_start_transcription_job_with_explicit_language() {
        use crate::client::{AudioConfig, AudioFormat, TranscriptionConfig};

        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-west-2";

        let mock_client = Arc::new(MockHttpClient::new());

        let mock_response = r#"{
                "TranscriptionJob": {
                    "TranscriptionJobName": "test-job-lang",
                    "TranscriptionJobStatus": "IN_PROGRESS"
                }
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(mock_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let audio_config = AudioConfig {
            format: AudioFormat::mp3,
            channels: Some(1),
        };

        let transcription_config = TranscriptionConfig {
            language: Some("en-US".to_string()),
            model: None,
            enable_speaker_diarization: false,
            vocabulary: vec![],
        };

        let _result = block_on(|_| async {
            client
                .start_transcription_job(
                    "test-job-lang".to_string(),
                    "s3://bucket/audio.mp3".to_string(),
                    audio_config,
                    Some(transcription_config),
                    None,
                )
                .await
        });

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartTranscriptionJobRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartTranscriptionJobRequest {
            content_redaction: None,
            identify_language: None, // Should be None when explicit language provided
            identify_multiple_languages: None,
            job_execution_settings: None,
            kms_encryption_context: None,
            language_code: Some("en-US".to_string()), // Should be set
            language_id_settings: None,
            language_options: None,
            media: Media {
                media_file_uri: "s3://bucket/audio.mp3".to_string(),
                redacted_media_file_uri: None,
            },
            media_format: Some("mp3".to_string()),
            media_sample_rate_hertz: None,
            model_settings: None, // No model specified
            output_bucket_name: None,
            output_encryption_kms_key_id: None,
            output_key: None,
            settings: None, // No settings should be set
            subtitles: None,
            tags: None,
            toxicity_detection: None,
            transcription_job_name: "test-job-lang".to_string(),
        };

        assert_eq!(
            actual_request, expected_request,
            "Request with explicit language should match expected structure"
        );
    }

    #[test]
    fn test_start_transcription_job_with_model_and_vocabulary() {
        use crate::client::{AudioConfig, AudioFormat, TranscriptionConfig};

        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "eu-west-1";

        let mock_client = Arc::new(MockHttpClient::new());

        let mock_response = r#"{
               "TranscriptionJob": {
                   "TranscriptionJobName": "test-job-advanced",
                   "TranscriptionJobStatus": "COMPLETED"
               }
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(mock_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let audio_config = AudioConfig {
            format: AudioFormat::flac,
            channels: Some(1),
        };

        let transcription_config = TranscriptionConfig {
            language: Some("fr-FR".to_string()),
            model: Some("custom-medical-model".to_string()),
            enable_speaker_diarization: false,
            vocabulary: vec!["A".to_string(), "B".to_string()],
        };

        let _result = block_on(|_| async {
            client
                .start_transcription_job(
                    "test-job-advanced".to_string(),
                    "s3://bucket/audio.flac".to_string(),
                    audio_config,
                    Some(transcription_config),
                    Some("custom-medical-vocab-123".to_string()), // Vocabulary name provided
                )
                .await
        });

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartTranscriptionJobRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartTranscriptionJobRequest {
            content_redaction: None,
            identify_language: None, // Explicit language provided
            identify_multiple_languages: None,
            job_execution_settings: None,
            kms_encryption_context: None,
            language_code: Some("fr-FR".to_string()),
            language_id_settings: None,
            language_options: None,
            media: Media {
                media_file_uri: "s3://bucket/audio.flac".to_string(),
                redacted_media_file_uri: None,
            },
            media_format: Some("flac".to_string()),
            media_sample_rate_hertz: None,
            model_settings: Some(ModelSettings {
                language_model_name: "custom-medical-model".to_string(),
            }),
            output_bucket_name: None,
            output_encryption_kms_key_id: None,
            output_key: None,
            settings: Some(Settings {
                channel_identification: None,
                max_alternatives: None,
                max_speaker_labels: None,
                show_alternatives: None,
                show_speaker_labels: None,
                vocabulary_filter_method: None,
                vocabulary_filter_name: None,
                vocabulary_name: Some("custom-medical-vocab-123".to_string()),
            }),
            subtitles: None,
            tags: None,
            toxicity_detection: None,
            transcription_job_name: "test-job-advanced".to_string(),
        };

        assert_eq!(
            actual_request, expected_request,
            "Request with model and vocabulary should match expected structure"
        );
    }

    #[test]
    fn test_start_transcription_job_with_speaker_diarization() {
        use crate::client::{AudioConfig, AudioFormat, TranscriptionConfig};

        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "ap-southeast-1";

        let mock_client = Arc::new(MockHttpClient::new());

        let mock_response = r#"{
                "TranscriptionJob": {
                    "TranscriptionJobName": "test-job-speakers",
                    "TranscriptionJobStatus": "IN_PROGRESS"
                }
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(mock_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let audio_config = AudioConfig {
            format: AudioFormat::ogg,
            channels: Some(1), // Single channel
        };

        let transcription_config = TranscriptionConfig {
            language: Some("en-AU".to_string()),
            model: None,
            enable_speaker_diarization: true, // Enable speaker diarization
            vocabulary: vec![],
        };

        let _result = block_on(|_| async {
            client
                .start_transcription_job(
                    "test-job-speakers".to_string(),
                    "s3://bucket/meeting.ogg".to_string(),
                    audio_config,
                    Some(transcription_config),
                    None,
                )
                .await
        });

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartTranscriptionJobRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartTranscriptionJobRequest {
            content_redaction: None,
            identify_language: None,
            identify_multiple_languages: None,
            job_execution_settings: None,
            kms_encryption_context: None,
            language_code: Some("en-AU".to_string()),
            language_id_settings: None,
            language_options: None,
            media: Media {
                media_file_uri: "s3://bucket/meeting.ogg".to_string(),
                redacted_media_file_uri: None,
            },
            media_format: Some("ogg".to_string()),
            media_sample_rate_hertz: None,
            model_settings: None,
            output_bucket_name: None,
            output_encryption_kms_key_id: None,
            output_key: None,
            settings: Some(Settings {
                channel_identification: None, // Single channel
                max_alternatives: None,
                max_speaker_labels: Some(30), // Set for speaker diarization
                show_alternatives: None,
                show_speaker_labels: Some(true), // Enable speaker labels
                vocabulary_filter_method: None,
                vocabulary_filter_name: None,
                vocabulary_name: None,
            }),
            subtitles: None,
            tags: None,
            toxicity_detection: None,
            transcription_job_name: "test-job-speakers".to_string(),
        };

        assert_eq!(
            actual_request, expected_request,
            "Request with speaker diarization should match expected structure"
        );
    }

    #[test]
    fn test_start_transcription_job_with_multi_channel() {
        use crate::client::{AudioConfig, AudioFormat, TranscriptionConfig};

        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "ca-central-1";

        let mock_client = Arc::new(MockHttpClient::new());

        let mock_response = r#"{
                "TranscriptionJob": {
                    "TranscriptionJobName": "test-job-channels",
                    "TranscriptionJobStatus": "COMPLETED"
                }
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(mock_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let audio_config = AudioConfig {
            format: AudioFormat::wav,
            channels: Some(2), // Multi-channel audio
        };

        let transcription_config = TranscriptionConfig {
            language: Some("en-CA".to_string()),
            model: Some("telephony-model".to_string()),
            enable_speaker_diarization: true, // Both multi-channel and speaker diarization
            vocabulary: vec!["A".to_string(), "B".to_string()],
        };

        let _result = block_on(|_| async {
            client
                .start_transcription_job(
                    "test-job-channels".to_string(),
                    "s3://bucket/call-recording.wav".to_string(),
                    audio_config,
                    Some(transcription_config),
                    Some("telephony-vocab".to_string()),
                )
                .await
        });

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartTranscriptionJobRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartTranscriptionJobRequest {
            content_redaction: None,
            identify_language: None,
            identify_multiple_languages: None,
            job_execution_settings: None,
            kms_encryption_context: None,
            language_code: Some("en-CA".to_string()),
            language_id_settings: None,
            language_options: None,
            media: Media {
                media_file_uri: "s3://bucket/call-recording.wav".to_string(),
                redacted_media_file_uri: None,
            },
            media_format: Some("wav".to_string()),
            media_sample_rate_hertz: None,
            model_settings: Some(ModelSettings {
                language_model_name: "telephony-model".to_string(),
            }),
            output_bucket_name: None,
            output_encryption_kms_key_id: None,
            output_key: None,
            settings: Some(Settings {
                channel_identification: Some(true), // Multi-channel
                max_alternatives: None,
                max_speaker_labels: Some(30), // Speaker diarization
                show_alternatives: None,
                show_speaker_labels: Some(true), // Speaker diarization
                vocabulary_filter_method: None,
                vocabulary_filter_name: None,
                vocabulary_name: Some("telephony-vocab".to_string()),
            }),
            subtitles: None,
            tags: None,
            toxicity_detection: None,
            transcription_job_name: "test-job-channels".to_string(),
        };

        assert_eq!(
            actual_request, expected_request,
            "Request with multi-channel and speaker diarization should match expected structure"
        );
    }

    #[test]
    fn test_start_transcription_job_identify_language_ignores_vocabulary_and_model_settings() {
        use crate::client::{AudioConfig, AudioFormat};

        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        let mock_response = r#"{
               "TranscriptionJob": {
                   "TranscriptionJobName": "test-job-identify",
                   "TranscriptionJobStatus": "IN_PROGRESS"
               }
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(mock_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        let audio_config = AudioConfig {
            format: AudioFormat::mp3,
            channels: Some(1),
        };

        let transcription_config = TranscriptionConfig {
            language: None, // means identify language
            model: Some("telephony-model".to_string()),
            enable_speaker_diarization: false,
            vocabulary: vec!["A".to_string(), "B".to_string()],
        };

        let _result = block_on(|_| async {
            client
                .start_transcription_job(
                    "test-job-identify".to_string(),
                    "s3://bucket/unknown-language.mp3".to_string(),
                    audio_config,
                    Some(transcription_config),
                    Some("some-vocab".to_string()), // This should be ignored
                )
                .await
        });

        let request = mock_client.last_captured_request().unwrap();
        let actual_request: StartTranscriptionJobRequest =
            serde_json::from_slice(request.body()).unwrap();

        let expected_request = StartTranscriptionJobRequest {
            content_redaction: None,
            identify_language: Some(true), // Should be true
            identify_multiple_languages: None,
            job_execution_settings: None,
            kms_encryption_context: None,
            language_code: None, // Should be None
            language_id_settings: None,
            language_options: None,
            media: Media {
                media_file_uri: "s3://bucket/unknown-language.mp3".to_string(),
                redacted_media_file_uri: None,
            },
            media_format: Some("mp3".to_string()),
            media_sample_rate_hertz: None,
            model_settings: None, // Should be None for identify_language
            output_bucket_name: None,
            output_encryption_kms_key_id: None,
            output_key: None,
            settings: None, // Vocabulary should be ignored, so settings should be None
            subtitles: None,
            tags: None,
            toxicity_detection: None,
            transcription_job_name: "test-job-identify".to_string(),
        };

        assert_eq!(
            actual_request, expected_request,
            "Request with identify_language should ignore vocabulary and model settings"
        );
    }

    #[test]
    fn test_wait_for_vocabulary_ready_success() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        // First call - vocabulary is still PENDING
        let pending_response = r#"{
               "VocabularyName": "test-vocab",
               "LanguageCode": "en-US",
               "VocabularyState": "PENDING",
               "LastModifiedTime": 1234567890.0
           }"#;

        // Second call - vocabulary is READY
        let ready_response = r#"{
               "VocabularyName": "test-vocab",
               "LanguageCode": "en-US",
               "VocabularyState": "READY",
               "LastModifiedTime": 1234567891.0,
               "DownloadUri": "s3://bucket/vocab.txt"
           }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(pending_response))
                .unwrap(),
        );

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(ready_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        // Mock runtime that tracks sleep calls
        struct MockRuntime {
            sleep_calls: std::cell::RefCell<Vec<Duration>>,
        }

        impl MockRuntime {
            fn new() -> Self {
                Self {
                    sleep_calls: std::cell::RefCell::new(Vec::new()),
                }
            }

            fn get_sleep_calls(&self) -> Vec<Duration> {
                self.sleep_calls.borrow().clone()
            }
        }

        impl golem_stt::runtime::AsyncRuntime for MockRuntime {
            async fn sleep(&self, duration: Duration) {
                self.sleep_calls.borrow_mut().push(duration);
            }
        }

        let mock_runtime = MockRuntime::new();

        let result = block_on(|_| async {
            client
                .wait_for_vocabulary_ready(&mock_runtime, "test-vocab", Duration::from_secs(300))
                .await
        });

        assert!(result.is_ok(), "wait_for_vocabulary_ready should succeed");

        assert_eq!(mock_client.captured_request_count(), 2);

        let sleep_calls = mock_runtime.get_sleep_calls();
        assert!(
            !sleep_calls.is_empty(),
            "Should have called sleep at least once"
        );
        assert_eq!(
            sleep_calls[0],
            Duration::from_millis(500),
            "First sleep should be 500ms"
        );

        let captured_requests = mock_client.get_captured_requests();
        for request in captured_requests.iter() {
            assert_eq!(request.method(), "POST");
            assert_eq!(
                request.headers().get("x-amz-target").unwrap(),
                "com.amazonaws.transcribe.Transcribe.GetVocabulary"
            );

            assert!(request.headers().contains_key("x-amz-date"));
            assert!(request.headers().contains_key("x-amz-content-sha256"));
            assert!(request.headers().contains_key("authorization"));
            assert!(request.headers().contains_key("host"));

            let auth_header = request
                .headers()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap();
            assert!(auth_header.starts_with("AWS4-HMAC-SHA256"));
            assert!(auth_header.contains("Credential="));
            assert!(auth_header.contains("SignedHeaders="));
            assert!(auth_header.contains("Signature="));

            let request_body: GetVocabularyRequest =
                serde_json::from_slice(request.body()).unwrap();
            assert_eq!(request_body.vocabulary_name, "test-vocab");
        }
    }

    #[test]
    fn test_wait_for_vocabulary_ready_failure() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        // Vocabulary creation failed
        let failed_response = r#"{
                "VocabularyName": "test-vocab",
                "LanguageCode": "en-US",
                "VocabularyState": "FAILED",
                "LastModifiedTime": 1234567890.0,
                "FailureReason": "Invalid vocabulary format"
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(failed_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        struct MockRuntime;
        impl golem_stt::runtime::AsyncRuntime for MockRuntime {
            async fn sleep(&self, _duration: Duration) {}
        }
        let mock_runtime = MockRuntime;

        let result = block_on(|_| async {
            client
                .wait_for_vocabulary_ready(&mock_runtime, "test-vocab", Duration::from_secs(300))
                .await
        });

        // Should fail with the specific error
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            golem_stt::error::Error::APIBadRequest {
                request_id,
                provider_error,
            } => {
                assert_eq!(request_id, "test-vocab");
                assert!(provider_error.contains("Invalid vocabulary format"));
            }
            _ => panic!("Expected APIBadRequest error"),
        }
    }

    #[test]
    fn test_wait_for_vocabulary_ready_timeout() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        // Always return PENDING to simulate timeout
        let pending_response = r#"{
                "VocabularyName": "test-vocab",
                "LanguageCode": "en-US",
                "VocabularyState": "PENDING",
                "LastModifiedTime": 1234567890.0
            }"#;

        // Add multiple responses to allow polling before timeout
        for _ in 0..100 {
            mock_client.expect_response(
                Response::builder()
                    .status(200)
                    .body(Bytes::from(pending_response))
                    .unwrap(),
            );
        }

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

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

        let result = block_on(|_| async {
            client
                .wait_for_vocabulary_ready(
                    &mock_runtime,
                    "test-vocab",
                    Duration::from_millis(5), // Very short timeout
                )
                .await
        });

        assert!(
            mock_runtime.elapsed_time.borrow().as_millis() > 0,
            "Elapsed time should be greater than zero"
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            golem_stt::error::Error::APIBadRequest {
                request_id,
                provider_error,
            } => {
                assert_eq!(request_id, "test-vocab");
                assert!(provider_error.contains("timed out"));
            }
            _ => panic!("Expected APIBadRequest timeout error"),
        }
    }

    #[test]
    fn test_wait_for_transcription_job_completion_success() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        // First call - job is IN_PROGRESS
        let in_progress_response = r#"{
                "TranscriptionJob": {
                    "TranscriptionJobName": "test-job",
                    "TranscriptionJobStatus": "IN_PROGRESS",
                    "LanguageCode": "en-US",
                    "Media": {
                        "MediaFileUri": "s3://bucket/audio.wav"
                    },
                    "MediaFormat": "wav",
                    "CreationTime": 1234567890.0,
                    "StartTime": 1234567891.0
                }
            }"#;

        // Second call - job is COMPLETED
        let completed_response = r#"{
                "TranscriptionJob": {
                    "TranscriptionJobName": "test-job",
                    "TranscriptionJobStatus": "COMPLETED",
                    "LanguageCode": "en-US",
                    "Media": {
                        "MediaFileUri": "s3://bucket/audio.wav"
                    },
                    "MediaFormat": "wav",
                    "CreationTime": 1234567890.0,
                    "CompletionTime": 1234567920.0,
                    "StartTime": 1234567891.0,
                    "Transcript": {
                        "TranscriptFileUri": "s3://output/transcript.json"
                    }
                }
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(in_progress_response))
                .unwrap(),
        );

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(completed_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        struct MockRuntime {
            sleep_calls: std::cell::RefCell<Vec<Duration>>,
        }

        impl MockRuntime {
            fn new() -> Self {
                Self {
                    sleep_calls: std::cell::RefCell::new(Vec::new()),
                }
            }

            fn get_sleep_calls(&self) -> Vec<Duration> {
                self.sleep_calls.borrow().clone()
            }
        }

        impl golem_stt::runtime::AsyncRuntime for MockRuntime {
            async fn sleep(&self, duration: Duration) {
                self.sleep_calls.borrow_mut().push(duration);
            }
        }

        let mock_runtime = MockRuntime::new();

        let result = block_on(|_| async {
            client
                .wait_for_transcription_job_completion(
                    &mock_runtime,
                    "test-job",
                    Duration::from_secs(3600),
                )
                .await
        });

        // Should succeed and return the completed job
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(
            response.transcription_job.transcription_job_status,
            "COMPLETED"
        );
        assert!(response.transcription_job.transcript.is_some());
        assert!(response
            .transcription_job
            .transcript
            .unwrap()
            .transcript_file_uri
            .is_some());

        // Should have made exactly 2 API calls
        assert_eq!(mock_client.captured_request_count(), 2);

        // Should have called sleep at least once
        let sleep_calls = mock_runtime.get_sleep_calls();
        assert!(!sleep_calls.is_empty());
        assert_eq!(
            sleep_calls[0],
            Duration::from_millis(2000),
            "First sleep should be 2000ms"
        );

        // Verify the requests were get_transcription_job calls
        let captured_requests = mock_client.get_captured_requests();
        for request in captured_requests.iter() {
            assert_eq!(request.method(), "POST");
            assert_eq!(
                request.headers().get("x-amz-target").unwrap(),
                "com.amazonaws.transcribe.Transcribe.GetTranscriptionJob"
            );

            assert!(request.headers().contains_key("x-amz-date"));
            assert!(request.headers().contains_key("x-amz-content-sha256"));
            assert!(request.headers().contains_key("authorization"));
            assert!(request.headers().contains_key("host"));

            let auth_header = request
                .headers()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap();
            assert!(auth_header.starts_with("AWS4-HMAC-SHA256"));
            assert!(auth_header.contains("Credential="));
            assert!(auth_header.contains("SignedHeaders="));
            assert!(auth_header.contains("Signature="));

            let request_body: GetTranscriptionJobRequest =
                serde_json::from_slice(request.body()).unwrap();
            assert_eq!(request_body.transcription_job_name, "test-job");
        }
    }

    #[test]
    fn test_wait_for_transcription_job_completion_failure() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        // Job failed
        let failed_response = r#"{
                "TranscriptionJob": {
                    "TranscriptionJobName": "test-job",
                    "TranscriptionJobStatus": "FAILED",
                    "LanguageCode": "en-US",
                    "Media": {
                        "MediaFileUri": "s3://bucket/audio.wav"
                    },
                    "MediaFormat": "wav",
                    "CreationTime": 1234567890.0,
                    "CompletionTime": 1234567920.0,
                    "StartTime": 1234567891.0,
                    "FailureReason": "Unsupported audio format"
                }
            }"#;

        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(Bytes::from(failed_response))
                .unwrap(),
        );

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

        struct MockRuntime;
        impl golem_stt::runtime::AsyncRuntime for MockRuntime {
            async fn sleep(&self, _duration: Duration) {}
        }
        let mock_runtime = MockRuntime;

        let result = block_on(|_| async {
            client
                .wait_for_transcription_job_completion(
                    &mock_runtime,
                    "test-job",
                    Duration::from_secs(3600),
                )
                .await
        });

        // Should fail with the specific error
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            golem_stt::error::Error::APIBadRequest {
                request_id,
                provider_error,
            } => {
                assert_eq!(request_id, "test-job");
                assert!(provider_error.contains("Unsupported audio format"));
            }
            _ => panic!("Expected APIBadRequest error"),
        }
    }

    #[test]
    fn test_wait_for_transcription_job_completion_timeout() {
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let mock_client = Arc::new(MockHttpClient::new());

        // Always return IN_PROGRESS to simulate timeout
        let in_progress_response = r#"{
            "TranscriptionJob": {
                "TranscriptionJobName": "test-job",
                "TranscriptionJobStatus": "IN_PROGRESS",
                "LanguageCode": "en-US",
                "Media": {
                    "MediaFileUri": "s3://bucket/audio.wav"
                },
                "MediaFormat": "wav",
                "CreationTime": 1234567890.0,
                "StartTime": 1234567891.0
            }
        }"#;

        // Add multiple responses to allow polling before timeout
        for _ in 0..100 {
            mock_client.expect_response(
                Response::builder()
                    .status(200)
                    .body(Bytes::from(in_progress_response))
                    .unwrap(),
            );
        }

        let client: TranscribeClient<MockHttpClient> = TranscribeClient::new(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
            mock_client.clone(),
        );

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

        let result = block_on(|_| async {
            client
                .wait_for_transcription_job_completion(
                    &mock_runtime,
                    "test-job",
                    Duration::from_millis(5), // Very short timeout
                )
                .await
        });

        assert!(
            mock_runtime.elapsed_time.borrow().as_millis() > 0,
            "Elapsed time should be greater than zero"
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            golem_stt::error::Error::APIBadRequest {
                request_id,
                provider_error,
            } => {
                assert_eq!(request_id, "test-job");
                assert!(provider_error.contains("timed out"));
            }
            _ => panic!("Expected APIBadRequest timeout error"),
        }
    }
}
