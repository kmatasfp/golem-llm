mod client;
mod conversions;

use crate::client::{AudioConfig, TranscriptionConfig, TranscriptionRequest, TranscriptionsApi};

use golem_stt::golem::stt::types::{
    AudioConfig as WitAudioConfig, SttError, TranscriptAlternative,
};
use golem_stt::http_client::ReqwestHttpClient;

use golem_stt::golem::stt::transcription::{
    Guest as TranscriptionGuest, GuestTranscriptionStream,
    TranscribeOptions as WitTranscribeOptions, TranscriptionResult as WitTranscriptionResult,
    TranscriptionStream as WitTranscriptionStream,
};

use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};

// Import for using common lib (also see Cargo.toml for adding the dependency):
// use common_lib::example_common_function;
use std::cell::RefCell;
use std::rc::Rc;

#[allow(unused)]
struct Component;

thread_local! {
    // Compile time: create empty container
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
    fn list_languages() -> Result<Vec<LanguageInfo>, SttError> {
        let api_client = get_client()
            .map_err(|_| SttError::InternalError("Api client should be available".to_string()))?;

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

struct WhisperTranscriptionStream {}

impl GuestTranscriptionStream for WhisperTranscriptionStream {
    fn send_audio(&self, _: Vec<u8>) -> Result<(), SttError> {
        Ok(())
    }

    fn finish(&self) -> Result<(), SttError> {
        Ok(())
    }

    fn receive_alternative(&self) -> Result<Option<TranscriptAlternative>, SttError> {
        Ok(None)
    }

    fn close(&self) {
        ()
    }
}

impl TranscriptionGuest for Component {
    type TranscriptionStream = WhisperTranscriptionStream;

    fn transcribe(
        audio: Vec<u8>,
        config: WitAudioConfig,
        options: Option<WitTranscribeOptions>,
    ) -> Result<WitTranscriptionResult, SttError> {
        let api_client = get_client().expect("api client should be available"); // Fixme: handle error

        let transcription_config: Option<TranscriptionConfig> = if let Some(options) = options {
            Some(options.try_into()?)
        } else {
            None
        };

        let request = TranscriptionRequest {
            audio,
            audio_config: AudioConfig {
                format: config.format.try_into()?,
            },
            transcription_config,
        };

        let api_response = api_client.transcribe_audio(request)?;

        Ok(api_response.into())
    }

    fn transcribe_stream(
        _: WitAudioConfig,
        _: Option<WitTranscribeOptions>,
    ) -> Result<WitTranscriptionStream, SttError> {
        Err(SttError::UnsupportedOperation(
            "Whisper model does not support streaming".to_string(),
        ))
    }
}

golem_stt::export_stt!(Component with_types_in golem_stt);
