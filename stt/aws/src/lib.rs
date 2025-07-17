use client::TranscribeApi;
use golem_stt::client::SttProviderClient;
use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};

use golem_stt::error::Error;

use golem_stt::golem::stt::transcription::{
    FailedTranscription as WitFailedTranscription, Guest as TranscriptionGuest,
    MultiTranscriptionResult as WitMultiTranscriptionResult,
    TranscriptionRequest as WitTranscriptionRequest, TranscriptionResult as WitTranscriptionResult,
};

use golem_stt::golem::stt::types::SttError as WitSttError;
use wasi_async_runtime::block_on;

mod aws;
mod aws_client;
mod client;
mod conversions;
mod error;

#[allow(unused)]
struct Component;

impl LanguageGuest for Component {
    fn list_languages() -> Result<Vec<LanguageInfo>, WitSttError> {
        let supported_languages = client::get_supported_languages();
        Ok(supported_languages
            .iter()
            .map(|lang| LanguageInfo {
                code: lang.code.to_string(),
                name: lang.name.to_string(),
                native_name: lang.native_name.to_string(),
            })
            .collect())
    }
}

impl TranscriptionGuest for Component {
    fn transcribe(req: WitTranscriptionRequest) -> Result<WitTranscriptionResult, WitSttError> {
        let region = std::env::var("AWS_REGION").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_REGION: {}", err))
        })?;

        let access_key = std::env::var("AWS_ACCESS_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_ACCESS_KEY: {}", err))
        })?;

        let secret_key = std::env::var("AWS_SECRET_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_SECRET_KEY: {}", err))
        })?;

        let bucket_name = std::env::var("AWS_BUCKET_NAME").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load AWS_BUCKET_NAME: {}", err))
        })?;

        block_on(|reactor| async {
            let api_client =
                TranscribeApi::live(bucket_name, access_key, secret_key, region, reactor);

            let api_response = api_client.transcribe_audio(req.try_into()?).await?;

            Ok(api_response.into())
        })
    }

    fn transcribe_many(
        wit_requests: Vec<WitTranscriptionRequest>,
    ) -> Result<WitMultiTranscriptionResult, WitSttError> {
        todo!()
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
