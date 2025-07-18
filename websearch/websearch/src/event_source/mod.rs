pub mod error;
mod event_stream;
mod ndjson_stream;
mod parser;
pub mod stream;
pub mod types;
mod utf8_stream;
use crate::event_source::event_stream::SseWebsearchStream;
use crate::event_source::stream::WebsearchStream;
use crate::event_source::types::WebsearchStreamEntry;
pub use error::StreamError;
use golem_rust::wasm_rpc::Pollable;
pub use ndjson_stream::NdJsonWebsearchStream;
pub use parser::{is_bom, is_lf, line, RawEventLine};
use reqwest::header::HeaderValue;
use reqwest::{Response, StatusCode};
use std::error::Error as StdError;
use std::task::Poll;
pub use stream::StreamType;
pub use types::{
    ImageResult, RateLimitInfo, SafeSearchLevel, SearchMetadata, SearchResult, StreamEnd,
};
pub use utf8_stream::Utf8Stream;

/// Represents connection state of an [`EventSource`]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum ReadyState {
    Connecting = 0,
    Open = 1,
    Closed = 2,
}

/// Wrapper over NDJSON or SSE streaming HTTP responses
pub struct EventSource {
    stream: StreamType,
    response: Response,
    is_closed: bool,
}

impl EventSource {
    /// Create a new [`EventSource`] from an HTTP response
    #[allow(clippy::result_large_err)]
    pub fn new(response: Response) -> Result<Self, Box<dyn StdError + Send + Sync>> {
        match check_response(response) {
            Ok(mut response) => {
                let handle = unsafe {
                    std::mem::transmute::<
                        reqwest::InputStream,
                        golem_rust::bindings::wasi::io::streams::InputStream,
                    >(response.get_raw_input_stream())
                };

                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                let stream = if content_type.contains("ndjson") {
                    StreamType::NdJsonStream(NdJsonWebsearchStream::new(handle))
                } else {
                    StreamType::EventStream(SseWebsearchStream::new(handle))
                };
                Ok(Self {
                    stream,
                    response,
                    is_closed: false,
                })
            }
            Err(err) => Err(err),
        }
    }

    /// Manually closes the stream
    pub fn close(&mut self) {
        self.is_closed = true;
    }

    /// Returns current state of stream
    pub fn ready_state(&self) -> ReadyState {
        if self.is_closed {
            ReadyState::Closed
        } else {
            ReadyState::Open
        }
    }

    /// Returns a `Pollable` object for event-driven readiness
    pub fn subscribe(&self) -> Pollable {
        match &self.stream {
            StreamType::EventStream(s) => s.subscribe(),
            StreamType::NdJsonStream(s) => s.subscribe(),
        }
    }

    /// Polls the next message from the stream
    pub fn poll_next(&mut self) -> Poll<Option<Result<Event, Box<dyn StdError + Send + Sync>>>> {
        if self.is_closed {
            return Poll::Ready(None);
        }

        match &mut self.stream {
            StreamType::EventStream(s) => match s.poll_next() {
                Poll::Ready(Some(Ok(event))) => {
                    Poll::Ready(Some(Ok(Event::Message(Box::new(event)))))
                }
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(Box::new(err)))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
            StreamType::NdJsonStream(s) => match s.poll_next() {
                Poll::Ready(Some(Ok(event))) => {
                    Poll::Ready(Some(Ok(Event::Message(Box::new(event)))))
                }
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(Box::new(err)))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

/// Top-level events emitted by EventSource
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Open,
    Message(Box<WebsearchStreamEntry>),
}

impl From<WebsearchStreamEntry> for Event {
    fn from(event: WebsearchStreamEntry) -> Self {
        Event::Message(Box::new(event))
    }
}

/// Custom error types for EventSource
#[derive(Debug)]
pub enum EventSourceError {
    InvalidStatusCode(StatusCode),
    InvalidContentType(HeaderValue),
}

impl std::fmt::Display for EventSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventSourceError::InvalidStatusCode(status) => {
                write!(f, "Invalid status code: {status}")
            }
            EventSourceError::InvalidContentType(content_type) => {
                write!(f, "Invalid content type: {content_type:?}")
            }
        }
    }
}

impl StdError for EventSourceError {}

/// Validate the HTTP response headers before accepting it as a stream
#[allow(clippy::result_large_err)]
fn check_response(response: Response) -> Result<Response, Box<dyn StdError + Send + Sync>> {
    match response.status() {
        StatusCode::OK => {}
        status => {
            return Err(Box::new(EventSourceError::InvalidStatusCode(status)));
        }
    }

    let content_type = response.headers().get(&reqwest::header::CONTENT_TYPE);

    let is_valid = content_type
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<mime::Mime>().ok())
        .map(|mime_type| {
            matches!(
                (mime_type.type_(), mime_type.subtype()),
                (mime::TEXT, mime::EVENT_STREAM)
            ) || mime_type.subtype().as_str().contains("ndjson")
        })
        .unwrap_or(false);

    if is_valid {
        Ok(response)
    } else {
        Err(Box::new(EventSourceError::InvalidContentType(
            content_type
                .cloned()
                .unwrap_or_else(|| HeaderValue::from_static("")),
        )))
    }
}
