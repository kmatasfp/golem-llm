use core::fmt;
use std::string::FromUtf8Error;
use thiserror::Error;
use reqwest::{ Error as ReqwestError, StatusCode };
use reqwest::header::HeaderValue;
use nom::error::Error as NomError;
use golem_rust::bindings::wasi::io::streams::{ StreamError as WasiStreamError };
use super::utf8_stream::Utf8StreamError;

/// Low-level streaming errors (UTF-8, parser, transport).
#[derive(Debug, PartialEq)]
pub enum StreamError<E> {
    Utf8(FromUtf8Error),
    Parser(NomError<String>),
    Transport(E),
}

/// High-level search errors returned by session logic or back-end adapter.
#[derive(Debug, Error)]
pub enum EventSourceSearchError {
    /// UTF-8 decoding failure in stream.
    #[error(transparent)]
    Utf8(FromUtf8Error),
    /// Protocol parser failure (SSE or NDJSON).
    #[error("Protocol parser error: {0}")]
    Parser(String), // Changed from NomError<String> to String
    /// HTTP-layer failure when issuing request.
    #[error("Transport error: {0}")]
    Transport(String), // Changed from ReqwestError to String
    /// Error while reading the streaming body.
    #[error("Transport stream error: {0}")]
    TransportStream(String),
    /// Invalid `Content-Type` from server.
    #[error("Invalid header value: {0}")]
    InvalidContentType(String), // Changed from HeaderValue to String
    /// Non-success HTTP status.
    #[error("Invalid status code: {0}")]
    InvalidStatusCode(u16), // Changed from StatusCode to u16
    /// Provided `Last-Event-ID` could not build header.
    #[error("Invalid `Last-Event-ID`: {0}")]
    InvalidLastEventId(String),
    /// The SSE/HTTP stream ended unexpectedly.
    #[error("Stream ended")]
    StreamEnded,
    /// Rate limiting (seconds until reset in WIT spec).
    #[error("Rate limited; retry after {0} s")]
    RateLimited(u32),
}

impl Clone for EventSourceSearchError {
    fn clone(&self) -> Self {
        match self {
            Self::Utf8(e) => Self::Utf8(e.clone()),
            Self::Parser(s) => Self::Parser(s.clone()),
            Self::Transport(s) => Self::Transport(s.clone()),
            Self::TransportStream(s) => Self::TransportStream(s.clone()),
            Self::InvalidContentType(s) => Self::InvalidContentType(s.clone()),
            Self::InvalidStatusCode(code) => Self::InvalidStatusCode(*code),
            Self::InvalidLastEventId(s) => Self::InvalidLastEventId(s.clone()),
            Self::StreamEnded => Self::StreamEnded,
            Self::RateLimited(secs) => Self::RateLimited(*secs),
        }
    }
}

impl From<ReqwestError> for EventSourceSearchError {
    fn from(err: ReqwestError) -> Self {
        Self::Transport(err.to_string())
    }
}

impl From<HeaderValue> for EventSourceSearchError {
    fn from(val: HeaderValue) -> Self {
        Self::InvalidContentType(val.to_str().unwrap_or("<invalid UTF-8>").to_string())
    }
}

impl From<StatusCode> for EventSourceSearchError {
    fn from(code: StatusCode) -> Self {
        Self::InvalidStatusCode(code.as_u16())
    }
}

impl From<NomError<String>> for EventSourceSearchError {
    fn from(err: NomError<String>) -> Self {
        Self::Parser(format!("Parse error at '{}': {:?}", err.input, err.code))
    }
}

impl From<StreamError<ReqwestError>> for EventSourceSearchError {
    fn from(e: StreamError<ReqwestError>) -> Self {
        match e {
            StreamError::Utf8(u) => Self::Utf8(u),
            StreamError::Parser(p) =>
                Self::Parser(format!("Parse error at '{}': {:?}", p.input, p.code)),
            StreamError::Transport(t) => Self::Transport(t.to_string()),
        }
    }
}

impl From<StreamError<WasiStreamError>> for EventSourceSearchError {
    fn from(e: StreamError<WasiStreamError>) -> Self {
        match e {
            StreamError::Utf8(u) => Self::Utf8(u),
            StreamError::Parser(p) =>
                Self::Parser(format!("Parse error at '{}': {:?}", p.input, p.code)),
            StreamError::Transport(t) =>
                match t {
                    WasiStreamError::Closed => Self::StreamEnded,
                    WasiStreamError::LastOperationFailed(inner) =>
                        Self::TransportStream(inner.to_debug_string()),
                }
        }
    }
}

impl<E> From<FromUtf8Error> for StreamError<E> {
    fn from(e: FromUtf8Error) -> Self {
        Self::Utf8(e)
    }
}

impl<E> From<NomError<&str>> for StreamError<E> {
    fn from(e: NomError<&str>) -> Self {
        Self::Parser(NomError::new(e.input.to_string(), e.code))
    }
}

impl<E> From<Utf8StreamError<E>> for StreamError<E> {
    fn from(e: Utf8StreamError<E>) -> Self {
        match e {
            Utf8StreamError::Utf8(e) => StreamError::Utf8(e),
            Utf8StreamError::Transport(e) => StreamError::Transport(e),
        }
    }
}

impl<E: fmt::Display> fmt::Display for StreamError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8(err) => write!(f, "UTF-8 error: {err}"),
            Self::Parser(err) => write!(f, "Parse error: {err}"),
            Self::Transport(err) => write!(f, "Transport error: {err}"),
        }
    }
}

impl<E> std::error::Error for StreamError<E> where E: fmt::Display + fmt::Debug + Send + Sync {}

// Implement conversion from EventSourceSearchError to the WIT-generated SearchError
impl From<EventSourceSearchError> for crate::exports::golem::web_search::web_search::SearchError {
    fn from(error: EventSourceSearchError) -> Self {
        match error {
            EventSourceSearchError::Utf8(_) => {
                Self::BackendError(format!("UTF-8 decoding error: {error}"))
            }
            EventSourceSearchError::Parser(_) => {
                Self::BackendError(format!("Protocol parser error: {error}"))
            }
            EventSourceSearchError::Transport(_) => {
                Self::BackendError(format!("HTTP transport error: {error}"))
            }
            EventSourceSearchError::TransportStream(_) => {
                Self::BackendError(format!("Transport stream error: {error}"))
            }
            EventSourceSearchError::InvalidContentType(_) => {
                Self::BackendError(format!("Invalid content type: {error}"))
            }
            EventSourceSearchError::InvalidStatusCode(_) => {
                Self::BackendError(format!("Invalid HTTP status: {error}"))
            }
            EventSourceSearchError::InvalidLastEventId(_) => { Self::InvalidQuery }
            EventSourceSearchError::StreamEnded => {
                Self::BackendError("Stream ended unexpectedly".to_string())
            }
            EventSourceSearchError::RateLimited(seconds) => { Self::RateLimited(seconds) }
        }
    }
}
