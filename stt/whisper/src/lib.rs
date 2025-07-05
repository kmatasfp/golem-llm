mod client;
mod conversions;

use crate::client::TranscriptionsApi;

use client::{TranscriptionRequest, TranscriptionResponse};
use golem_stt::client::{ReqwestHttpClient, SttProviderClient};
use golem_stt::error::Error;
use golem_stt::golem::stt::types::SttError as WitSttError;

use golem_stt::golem::stt::transcription::{
    Guest as TranscriptionGuest, GuestTranscriptionQueue,
    TranscriptionQueue as WitTranscriptionQueue, TranscriptionRequest as WitTranscriptionRequest,
    TranscriptionResult as WitTranscriptionResult,
};

use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};
use golem_stt::transcription_queue::TranscriptionQueue;

use std::cell::RefCell;
use std::sync::OnceLock;

#[allow(unused)]
struct Component;

// static CLIENT: OnceLock<TranscriptionsApi<ReqwestHttpClient>> = OnceLock::new();

// fn get_client() -> &'static TranscriptionsApi<ReqwestHttpClient> {
//     CLIENT.get_or_init(|| {
//         let api_key =
//             std::env::var("OPENAI_API_KEY").expect("env variable OPENAI_API_KEY was not set");
//         TranscriptionsApi::live(api_key)
//     })
// }

// impl LanguageGuest for Component {
//     fn list_languages() -> Result<Vec<LanguageInfo>, WitSttError> {
//         let api_client = get_client();

//         let supported_languages = api_client.get_supported_languages();
//         Ok(supported_languages
//             .iter()
//             .map(|lang| LanguageInfo {
//                 code: lang.code.to_string(),
//                 name: lang.name.to_string(),
//                 native_name: lang.native_name.to_string(),
//             })
//             .collect())
//     }
// }

// struct WhisperTranscriptionQueue {
//     queue: RefCell<
//         TranscriptionQueue<
//             'static,
//             TranscriptionsApi<ReqwestHttpClient>,
//             TranscriptionRequest,
//             TranscriptionResponse,
//             Error,
//         >,
//     >,
// }

// impl GuestTranscriptionQueue for WhisperTranscriptionQueue {
//     fn get_next(&self) -> Option<Result<WitTranscriptionResult, WitSttError>> {
//         self.queue.borrow_mut().get_next().map(|result| {
//             result
//                 .map(|transcription| transcription.into())
//                 .map_err(|e| e.into())
//         })
//     }

//     fn blocking_get_next(&self) -> Vec<Result<WitTranscriptionResult, WitSttError>> {
//         self.queue
//             .borrow_mut()
//             .blocking_get_next()
//             .into_iter()
//             .map(|result| {
//                 result
//                     .map(|transcription| transcription.into())
//                     .map_err(|e| e.into())
//             })
//             .collect()
//     }
// }

// impl TranscriptionGuest for Component {
//     type TranscriptionQueue = WhisperTranscriptionQueue;

//     fn transcribe(req: WitTranscriptionRequest) -> Result<WitTranscriptionResult, WitSttError> {
//         let api_client = get_client();

//         let api_response = api_client.transcribe_audio(req.try_into()?)?;

//         Ok(api_response.into())
//     }

//     fn queue_transcription(
//         requests: Vec<WitTranscriptionRequest>,
//     ) -> Result<WitTranscriptionQueue, WitSttError> {
//         let api_client = get_client();

//         let reqs: Result<Vec<TranscriptionRequest>, WitSttError> = requests
//             .into_iter()
//             .map(|req| req.try_into())
//             .try_fold(Vec::new(), |mut acc, res| {
//                 let item = res?;
//                 acc.push(item);
//                 Ok(acc)
//             });

//         let queue = TranscriptionQueue::new(api_client, reqs?);

//         Ok(WitTranscriptionQueue::new(WhisperTranscriptionQueue {
//             queue: queue.into(),
//         }))
//     }
// }

// golem_stt::export_stt!(Component with_types_in golem_stt);
