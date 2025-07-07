use bytes::Bytes;
use derive_more::From;
use http::{Request, Response};
use reqwest::Client;

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

pub trait HttpClient {
    fn execute(&self, request: Request<Bytes>) -> Result<Response<Bytes>, Error>;
}

pub struct ReqwestHttpClient {
    client: Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .build()
            .expect("Failed to initialize HTTP client");
        Self { client }
    }
}

impl HttpClient for ReqwestHttpClient {
    fn execute(&self, request: Request<Bytes>) -> Result<Response<Bytes>, Error> {
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
        let reqwest_response = self.client.execute(reqwest_request)?;

        let status = reqwest_response.status();
        let headers = reqwest_response.headers().clone();
        let body = reqwest_response.bytes()?;

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

pub trait SttProviderClient<REQ, RES, ERR: std::error::Error> {
    fn transcribe_audio(&self, request: REQ) -> Result<RES, ERR>;
}
