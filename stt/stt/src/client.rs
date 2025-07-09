use bytes::Bytes;
use derive_more::From;
use http::{Request, Response};
use reqwest::Client;
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

impl HttpClient for ReqwestHttpClient {
    async fn execute(&self, request: Request<Bytes>) -> Result<Response<Bytes>, Error> {
        fn to_reqwest_request(
            client: &Client,
            req: Request<Bytes>,
        ) -> Result<reqwest::Request, Error> {
            let (parts, body) = req.into_parts();

            let builder = client
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .version(parts.version)
                .body(body);

            builder.build().map_err(Error::Reqwest)
        }

        let reqwest_request = to_reqwest_request(&self.client, request)?;
        let reqwest_response = self.client.execute(reqwest_request).await?;

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
