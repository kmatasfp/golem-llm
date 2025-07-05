use bytes::Bytes;
use http::{Request, Response};
use reqwest::Client;

use crate::error::Error;

pub trait HttpClient {
    fn execute(&self, request: Request<Bytes>) -> Result<Response<Bytes>, Error>;
}

pub struct ReqwestHttpClient2 {
    client: Client,
}

impl ReqwestHttpClient2 {
    pub fn new() -> Self {
        let client = Client::builder()
            .build()
            .expect("Failed to initialize HTTP client");
        Self { client }
    }
}

impl HttpClient for ReqwestHttpClient2 {
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
