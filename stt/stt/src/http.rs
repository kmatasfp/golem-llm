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

pub struct MultipartBuilder {
    boundary: String,
    parts: Vec<Vec<u8>>,
}

impl MultipartBuilder {
    pub fn new() -> Self {
        Self {
            boundary: format!("----formdata-{}", uuid::Uuid::new_v4()),
            parts: Vec::new(),
        }
    }

    pub fn add_bytes(&mut self, name: &str, filename: &str, content_type: &str, data: Vec<u8>) {
        self.parts.push(format!(
                   "--{}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                   self.boundary, name, filename, content_type
               ).into_bytes());
        self.parts.push(data);
        self.parts.push(b"\r\n".to_vec());
    }

    pub fn add_field(&mut self, name: &str, value: &str) {
        self.parts.push(
            format!(
                "--{}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n",
                self.boundary, name, value
            )
            .into_bytes(),
        );
    }

    pub fn finish(mut self) -> (String, Vec<u8>) {
        // Add end boundary
        self.parts
            .push(format!("--{}--\r\n", self.boundary).into_bytes());
        // Calculate total size and build final buffer
        let total_size: usize = self.parts.iter().map(|b| b.len()).sum();
        let mut final_buffer = Vec::with_capacity(total_size);

        for part in self.parts {
            final_buffer.extend(part);
        }

        let content_type = format!("multipart/form-data; boundary={}", self.boundary);
        (content_type, final_buffer)
    }
}
