use bytes::Bytes;
use derive_more::From;
use http::{Request, Response};
use reqwest::Client;
use url::Url;
use wasi_async_runtime::Reactor;

#[allow(unused)]
#[derive(Debug, From)]
pub enum Error {
    #[from]
    HttpError(http::Error),
    #[from]
    Reqwest(reqwest::Error),
    #[from]
    Io(std::io::Error),
    Generic(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[allow(async_fn_in_trait)]
pub trait HttpClient {
    async fn execute(&self, request: Request<Bytes>) -> Result<Response<Bytes>, Error>;
}

pub struct ReqwestHttpClient {
    client: Client,
}

impl ReqwestHttpClient {
    pub fn new(reactor: Reactor) -> Self {
        let client = Client::new(reactor);
        Self { client }
    }
}

struct WasiRequest(reqwest::Request);

impl From<Request<Bytes>> for WasiRequest {
    fn from(request: Request<Bytes>) -> Self {
        let (parts, body) = request.into_parts();
        let url = Url::parse(&parts.uri.to_string()).expect("Valid URL");

        let mut req = reqwest::Request::new(parts.method, url);
        *req.headers_mut() = parts.headers;
        *req.version_mut() = parts.version;
        *req.body_mut() = Some(body.into());
        WasiRequest(req)
    }
}

impl HttpClient for ReqwestHttpClient {
    async fn execute(&self, request: Request<Bytes>) -> Result<Response<Bytes>, Error> {
        let reqwest_request = WasiRequest::from(request);
        let reqwest_response = self.client.execute(reqwest_request.0).await?;

        let status = reqwest_response.status();
        let headers = reqwest_response.headers().clone();
        let body = reqwest_response.bytes().await?;

        let mut response = Response::builder().status(status).body(body).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Failed to build response: {}", e),
            ))
        })?;

        *response.headers_mut() = headers;

        Ok(response)
    }
}

#[allow(async_fn_in_trait)]
pub trait SttProviderClient<REQ, RES, ERR: std::error::Error> {
    async fn transcribe_audio(&self, request: REQ) -> Result<RES, ERR>;
}
