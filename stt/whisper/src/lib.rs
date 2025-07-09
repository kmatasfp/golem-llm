mod client;
mod conversions;

use crate::client::TranscriptionsApi;
use itertools::Itertools;

use client::TranscriptionRequest;
use golem_stt::client::SttProviderClient;
use golem_stt::error::Error;
use golem_stt::golem::stt::types::SttError as WitSttError;

use golem_stt::golem::stt::transcription::{
    FailedTranscription as WitFailedTranscription, Guest as TranscriptionGuest,
    MultiTranscriptionResult as WitMultiTranscriptionResult,
    TranscriptionRequest as WitTranscriptionRequest, TranscriptionResult as WitTranscriptionResult,
};

use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};
use wasi_async_runtime::block_on;

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
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load OPENAI_API_KEY: {}", err))
        })?;

        block_on(|reactor| async {
            let api_client = TranscriptionsApi::live(api_key, reactor);

            let api_response = api_client.transcribe_audio(req.try_into()?).await?;

            Ok(api_response.into())
        })
    }

    fn transcribe_many(
        wit_requests: Vec<WitTranscriptionRequest>,
    ) -> Result<WitMultiTranscriptionResult, WitSttError> {
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|err| {
            Error::EnvVariablesNotSet(format!("Failed to load OPENAI_API_KEY: {}", err))
        })?;

        block_on(|reactor| async {
            let api_client = TranscriptionsApi::live(api_key, reactor);

            let mut successes: Vec<WitTranscriptionResult> = Vec::new();
            let mut failures: Vec<WitFailedTranscription> = Vec::new();

            let requests: Vec<_> = wit_requests
                .into_iter()
                .map(|wr| (wr.request_id.clone(), TranscriptionRequest::try_from(wr)))
                .filter_map(|(id, res)| match res {
                    Ok(req) => Some(req),
                    Err(err) => {
                        failures.push(WitFailedTranscription {
                            request_id: id,
                            error: err,
                        });
                        None
                    }
                })
                .collect();

            for chunk in requests.into_iter().chunks(16).into_iter() {
                let req_vec: Vec<_> = chunk.collect();

                let futures = req_vec
                    .into_iter()
                    .map(|request| api_client.transcribe_audio(request))
                    .collect::<Vec<_>>();

                let results = futures::future::join_all(futures).await;

                for res in results {
                    match res {
                        Ok(resp) => successes.push(resp.into()),
                        Err(err) => failures.push(WitFailedTranscription {
                            request_id: err.request_id().to_string(),
                            error: WitSttError::from(err),
                        }),
                    }
                }
            }

            Ok(WitMultiTranscriptionResult {
                successes,
                failures,
            })
        })
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
