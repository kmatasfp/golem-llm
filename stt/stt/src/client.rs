use bytes::Bytes;
use reqwest::{Client, IntoUrl, Method, Request, RequestBuilder};

use crate::error::Error;

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    body: Bytes,
}

#[allow(unused)]
impl HttpResponse {
    pub fn new(status: u16, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn bytes(&self) -> &Bytes {
        &self.body
    }

    pub fn text(self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.body.to_vec())
    }

    pub fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }
}

pub trait HttpClient {
    fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder;
    fn execute(&self, request: Request) -> Result<HttpResponse, Error>;
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
    fn execute(&self, request: Request) -> Result<HttpResponse, Error> {
        let response = self.client.execute(request)?;
        let status = response.status().as_u16();
        let body = response.bytes()?;
        Ok(HttpResponse { status, body })
    }

    fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        self.client.request(method, url)
    }
}

pub trait SttProviderClient<Req, Res, Err: std::error::Error> {
    fn transcribe_audio(&self, request: Req) -> Result<Res, Err>;
}
