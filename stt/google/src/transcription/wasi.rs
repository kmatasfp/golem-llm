use golem_stt::{http::WstdHttpClient, runtime::WasiAyncRuntime};

use super::{
    api::SpeechToTextApi, gcp_cloud_storage::CloudStorageClient,
    gcp_speech_to_text::SpeechToTextClient,
};

impl
    SpeechToTextApi<
        CloudStorageClient<WstdHttpClient>,
        SpeechToTextClient<WstdHttpClient, WasiAyncRuntime>,
    >
{
    pub fn live() -> Self {
        todo!()
    }
}
