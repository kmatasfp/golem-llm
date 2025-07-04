mod client;
mod conversions;

use crate::client::TranscriptionsApi;

use golem_stt::client::{ReqwestHttpClient, SttProviderClient};
use golem_stt::golem::stt::types::SttError as WitSttError;

use golem_stt::golem::stt::transcription::{
    Guest as TranscriptionGuest, GuestTranscriptionQueue,
    TranscriptionQueue as WitTranscriptionQueue, TranscriptionRequest as WitTranscriptionRequest,
    TranscriptionResult as WitTranscriptionResult,
};

use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};

// Import for using common lib (also see Cargo.toml for adding the dependency):
// use common_lib::example_common_function;
use std::cell::RefCell;
use std::rc::Rc;

#[allow(unused)]
struct Component;

thread_local! {
    static CLIENT_CACHE: RefCell<Option<Rc<TranscriptionsApi<ReqwestHttpClient>>>> = const { RefCell::new(None) };
}

fn get_client() -> Result<Rc<TranscriptionsApi<ReqwestHttpClient>>, String> {
    CLIENT_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();

        match cache_ref.as_ref() {
            Some(client) => Ok(client.clone()),
            None => {
                let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "".to_string());

                let client = Rc::new({
                    let openai_api_key = api_key;
                    TranscriptionsApi::live(openai_api_key)
                });
                *cache_ref = Some(client.clone());
                Ok(client)
            }
        }
    })
}

impl LanguageGuest for Component {
    fn list_languages() -> Result<Vec<LanguageInfo>, WitSttError> {
        let api_client = get_client().map_err(|_| {
            WitSttError::InternalError("Api client should be available".to_string())
        })?;

        let supported_languages = api_client.get_supported_languages();
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

struct WhisperTranscriptionQueue {}

impl GuestTranscriptionQueue for WhisperTranscriptionQueue {
    fn get_next(&self) -> Option<WitTranscriptionResult> {
        todo!()
    }

    fn blocking_get_next(&self) -> Vec<WitTranscriptionResult> {
        todo!()
    }
}

impl TranscriptionGuest for Component {
    type TranscriptionQueue = WhisperTranscriptionQueue;

    fn transcribe(req: WitTranscriptionRequest) -> Result<WitTranscriptionResult, WitSttError> {
        let api_client = get_client().expect("api client should be available"); // Fixme: handle error

        let api_response = api_client.transcribe_audio(req.try_into()?)?;

        Ok(api_response.into())
    }

    fn queue_transcription(_requests: Vec<WitTranscriptionRequest>) -> WitTranscriptionQueue {
        todo!()
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
