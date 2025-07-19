use derive_more::From;
use http::{Request, Response};
use wstd::{
    http::{body::BoundedBody, Client, IntoBody},
    time::Duration,
};

#[allow(unused)]
#[derive(Debug, From)]
pub enum Error {
    #[from]
    HttpError(http::Error),
    #[from]
    WstdHttpError(wstd::http::Error),
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
    async fn execute(&self, request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, Error>;
}

pub struct WstdHttpClient {
    client: Client,
}

impl WstdHttpClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub fn new_with_timeout(connection_timeout: Duration, first_byte_timeout: Duration) -> Self {
        let mut client = Client::new();
        client.set_connect_timeout(connection_timeout);
        client.set_first_byte_timeout(first_byte_timeout);

        Self { client }
    }
}

impl Default for WstdHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

struct WasiRequest(wstd::http::Request<BoundedBody<Vec<u8>>>);

impl From<Request<Vec<u8>>> for WasiRequest {
    fn from(request: Request<Vec<u8>>) -> Self {
        let (parts, body) = request.into_parts();

        let mut req = wstd::http::Request::builder()
            .uri(parts.uri)
            .method(parts.method)
            .version(parts.version)
            .body(body.into_body())
            .expect("Known valid");

        *req.headers_mut() = parts.headers;

        WasiRequest(req)
    }
}

impl HttpClient for WstdHttpClient {
    async fn execute(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> Result<http::Response<Vec<u8>>, Error> {
        let wasi_request = WasiRequest::from(request).0;

        let mut wasi_response = self.client.send(wasi_request).await?;

        let status = wasi_response.status();
        let headers = wasi_response.headers().clone();
        let body = wasi_response.body_mut().bytes().await?;

        let mut response = Response::builder().status(status).body(body)?;

        *response.headers_mut() = headers;

        Ok(response)
    }
}
