use golem_stt::{http::WstdHttpClient, runtime::WasiAyncRuntime};

use super::{api::TranscribeApi, aws_s3::S3Client, aws_transcribe::TranscribeClient};

impl TranscribeApi<S3Client<WstdHttpClient>, TranscribeClient<WstdHttpClient, WasiAyncRuntime>> {
    pub fn live(
        bucket_name: String,
        access_key: String,
        secret_key: String,
        region: String,
    ) -> Self {
        let s3_client = S3Client::new(
            access_key.clone(),
            secret_key.clone(),
            region.clone(),
            WstdHttpClient::default(),
        );

        let transcribe_client = TranscribeClient::new(
            access_key.clone(),
            secret_key.clone(),
            region.clone(),
            WstdHttpClient::default(),
            WasiAyncRuntime::new(),
        );

        Self::new(bucket_name, s3_client, transcribe_client)
    }
}
