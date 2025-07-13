use std::{
    cmp,
    fmt::Debug,
    future::Future,
    io::Read,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use aws_sdk_s3::{
    config::{AsyncSleep, RuntimeComponents, Sleep},
    error::ConnectorError,
};
use aws_smithy_runtime_api::client::http::{
    HttpClient as AwsSdkHttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings,
    SharedHttpConnector,
};
use aws_smithy_runtime_api::http::Response as AwsResponse;
use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use derive_more::From;
use http::{Request, Response};
use reqwest::{Body, Client};
use url::Url;
use wasi::clocks::monotonic_clock;
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
pub trait HttpClient<T> {
    async fn execute(
        &self,
        request: Request<T>,
        timeout: Option<Duration>,
    ) -> Result<Response<Bytes>, Error>;
}

#[derive(Debug, Clone)]
pub struct ReqwestAwsHttpClient {
    client: Client,
}

impl ReqwestAwsHttpClient {
    pub fn new(reactor: Reactor) -> Self {
        let client = Client::new(reactor);
        Self { client }
    }
}

struct WasiRequest(reqwest::Request);

struct BodyReader {
    body: SdkBody,
}

impl BodyReader {
    fn new(body: SdkBody) -> Self {
        Self { body }
    }
}

impl Read for BodyReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let body_bytes = self.body.bytes();

        match body_bytes {
            Some(mut bytes) => bytes.read(buf),
            None => Ok(0),
        }
    }
}

impl From<Request<SdkBody>> for WasiRequest {
    fn from(request: Request<SdkBody>) -> Self {
        let (parts, body) = request.into_parts();
        let url = Url::parse(&parts.uri.to_string()).expect("Valid URL");

        let mut req = reqwest::Request::new(parts.method, url);
        *req.headers_mut() = parts.headers;
        *req.version_mut() = parts.version;
        *req.body_mut() = Some(Body::new(BodyReader::new(body)));
        WasiRequest(req)
    }
}

impl HttpClient<SdkBody> for ReqwestAwsHttpClient {
    async fn execute(
        &self,
        request: Request<SdkBody>,
        timeout: Option<Duration>,
    ) -> Result<Response<Bytes>, Error> {
        let wasi_request = WasiRequest::from(request);
        let mut reqwest_request = wasi_request.0;

        if let Some(timeout) = timeout {
            *reqwest_request.timeout_mut() = Some(timeout);
        }

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

#[derive(Debug, Clone)]
pub struct WasiSleep {
    reactor: Reactor,
}

// we are strictly single threaded env
unsafe impl Send for WasiSleep {}
unsafe impl Sync for WasiSleep {}

impl WasiSleep {
    pub fn new(reactor: Reactor) -> Self {
        Self { reactor }
    }
}

pub struct UnsafeFuture<Fut> {
    inner: Fut,
}

impl<Fut> UnsafeFuture<Fut> {
    pub fn new(fut: Fut) -> Self {
        Self { inner: fut }
    }
}

unsafe impl<Fut> Send for UnsafeFuture<Fut> {}
unsafe impl<Fut> Sync for UnsafeFuture<Fut> {}

impl<Fut: Future> Future for UnsafeFuture<Fut> {
    type Output = Fut::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let inner = self.map_unchecked_mut(|s| &mut s.inner);
            inner.poll(cx)
        }
    }
}

impl AsyncSleep for WasiSleep {
    fn sleep(&self, duration: std::time::Duration) -> Sleep {
        let reactor = self.reactor.clone();

        let fut = async move {
            let duration = duration.as_nanos() as u64;
            let pollable = monotonic_clock::subscribe_duration(duration);
            reactor.clone().wait_for(pollable).await;
        };

        Sleep::new(Box::pin(UnsafeFuture::new(fut)))
    }
}

#[derive(Debug, Clone)]
pub struct AWSHttpClient<HC: HttpClient<SdkBody>> {
    client: Arc<HC>,
}

impl AWSHttpClient<ReqwestAwsHttpClient> {
    pub fn live(reactor: Reactor) -> Self {
        Self {
            client: Arc::new(ReqwestAwsHttpClient::new(reactor)),
        }
    }
}

unsafe impl<HC: HttpClient<SdkBody>> Send for AWSHttpClient<HC> {}
unsafe impl<HC: HttpClient<SdkBody>> Sync for AWSHttpClient<HC> {}

impl<HC: HttpClient<SdkBody> + Debug + 'static> AwsSdkHttpClient for AWSHttpClient<HC> {
    fn http_connector(
        &self,
        settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        let read_timeout = settings.read_timeout().unwrap_or(Duration::from_millis(0));
        let connect_timeout = settings
            .connect_timeout()
            .unwrap_or(Duration::from_millis(0));

        let longer_timeout = cmp::max(read_timeout, connect_timeout);

        let client = self.client.clone();
        let timeout = Arc::new(if longer_timeout > Duration::ZERO {
            Some(longer_timeout)
        } else {
            None
        });

        let connector = AWSHttpConnector { client, timeout };

        SharedHttpConnector::new(connector)
    }
}

#[derive(Debug, Clone)]
struct AWSHttpConnector<HC: HttpClient<SdkBody>> {
    client: Arc<HC>,
    timeout: Arc<Option<std::time::Duration>>,
}

unsafe impl<HC: HttpClient<SdkBody>> Send for AWSHttpConnector<HC> {}
unsafe impl<HC: HttpClient<SdkBody>> Sync for AWSHttpConnector<HC> {}

impl<HC: HttpClient<SdkBody> + Debug + 'static> HttpConnector for AWSHttpConnector<HC> {
    fn call(
        &self,
        request: aws_sdk_s3::config::http::HttpRequest,
    ) -> aws_smithy_runtime_api::client::http::HttpConnectorFuture {
        let http_req = request.try_into_http1x().expect("Http request invalid");

        let client = self.client.clone();
        let timeout = self.timeout.clone();

        let eventual_response = async move {
            let fut = client
                .execute(http_req, *timeout)
                .await
                .map_err(|err| ConnectorError::other(err.into(), None))?;
            let response = fut.map(|body| {
                if body.is_empty() {
                    SdkBody::empty()
                } else {
                    SdkBody::from(body)
                }
            });

            let sdk_res = AwsResponse::try_from(response)
                .map_err(|err| ConnectorError::other(err.into(), None))?;

            Ok(sdk_res)
        };

        HttpConnectorFuture::new(UnsafeFuture::new(eventual_response))
    }
}
