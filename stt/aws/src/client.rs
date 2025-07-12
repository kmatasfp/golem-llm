use std::{fmt::format, rc::Rc, sync::Arc, time::Duration};

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

        let object_key = format!("{}/audio.{}", request_id, request.audio_config.format);

        self.s3_client
            .put_object(self.bucket_name.as_ref(), &object_key, request.audio)
            .await
            .map_err(|err| Error::Client(request_id.to_string(), err))?;

        if let Some(transcription_config) = request.transcription_config {
            if transcription_config.vocabulary.len() > 0 {
                if transcription_config.language.is_none() {
                    return Err(Error::APIBadRequest {
                        request_id: request_id.to_string(),
                        provider_error:
                            "When specifying a vocabulary, a language must also be specified."
                                .to_string(),
                    });
                }
                let language_code = transcription_config.language.unwrap();

                let res = self
                    .transcribe_client
                    .create_vocabulary(
                        request_id.to_string(),
                        language_code,
                        transcription_config.vocabulary,
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
            }
            // TODO handle case if user specified some config options
            todo!()
        } else {
            let res = self
                .transcribe_client
                .start_transcription_job(
                    request_id.to_string(),
                    format!("s3://{}/{object_key}", self.bucket_name),
                    None,
                    request.audio_config.format.to_string(),
                    None,
                    None,
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
