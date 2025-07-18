use core::fmt;
use std::{string::FromUtf8Error, task::Poll};

use super::{
    event_stream::SseWebsearchStream, ndjson_stream::NdJsonWebsearchStream,
    utf8_stream::Utf8StreamError,
};
use crate::event_source::error::StreamError as ImportedStreamError;
use crate::event_source::types::WebsearchStreamEntry;
use golem_rust::bindings::wasi::io::streams::InputStream;
use golem_rust::wasm_rpc::Pollable;
use nom::error::Error as NomError;

/// Concrete stream variants we can wrap.
pub enum StreamType {
    EventStream(SseWebsearchStream),
    NdJsonStream(NdJsonWebsearchStream),
}

/// Trait implemented by both `EventStream` and `NdJsonStream`.
/// This trait is designed to be dyn-compatible (object-safe).
pub trait WebsearchStream {
    /// Item type yielded on success.
    type Item;
    /// Transport-level error type.
    type Error;

    /// `Last-Event-ID` header for resuming streams (SSE only).
    fn set_last_event_id_str(&mut self, id: String);
    fn last_event_id(&self) -> &str;
    /// Subscribe for async readiness.
    fn subscribe(&self) -> Pollable;
    /// Poll next item.
    fn poll_next(&mut self) -> Poll<Option<Result<Self::Item, Self::Error>>>;
}

/// Factory trait for creating streams from WASI InputStreams.
/// This separates construction from the main trait to maintain dyn-compatibility.
pub trait WebsearchStreamFactory {
    type Stream: WebsearchStream;

    fn new(stream: InputStream) -> Self::Stream;
}

/// Enum wrapper for different stream types to make them object-safe
pub enum WebsearchStreamType {
    Sse(
        Box<
            dyn WebsearchStream<
                Item = WebsearchStreamEntry,
                Error = ImportedStreamError<golem_rust::bindings::wasi::io::streams::StreamError>,
            >,
        >,
    ),
    NdJson(
        Box<
            dyn WebsearchStream<
                Item = WebsearchStreamEntry,
                Error = ImportedStreamError<golem_rust::bindings::wasi::io::streams::StreamError>,
            >,
        >,
    ),
}

impl WebsearchStream for WebsearchStreamType {
    type Item = WebsearchStreamEntry;
    type Error = ImportedStreamError<golem_rust::bindings::wasi::io::streams::StreamError>;

    fn poll_next(&mut self) -> Poll<Option<Result<Self::Item, Self::Error>>> {
        match self {
            WebsearchStreamType::Sse(stream) => stream.poll_next(),
            WebsearchStreamType::NdJson(stream) => stream.poll_next(),
        }
    }

    fn subscribe(&self) -> Pollable {
        match self {
            WebsearchStreamType::Sse(stream) => stream.subscribe(),
            WebsearchStreamType::NdJson(stream) => stream.subscribe(),
        }
    }

    fn last_event_id(&self) -> &str {
        match self {
            WebsearchStreamType::Sse(stream) => stream.last_event_id(),
            WebsearchStreamType::NdJson(stream) => stream.last_event_id(),
        }
    }

    fn set_last_event_id_str(&mut self, id: String) {
        match self {
            WebsearchStreamType::Sse(stream) => stream.set_last_event_id_str(id),
            WebsearchStreamType::NdJson(stream) => stream.set_last_event_id_str(id),
        }
    }
}

impl WebsearchStreamType {
    /// Create a new SSE stream
    pub fn new_sse(stream: InputStream) -> Self {
        Self::Sse(Box::new(SseWebsearchStream::new(stream)))
    }

    /// Create a new NDJSON stream
    pub fn new_ndjson(stream: InputStream) -> Self {
        Self::NdJson(Box::new(NdJsonWebsearchStream::new(stream)))
    }
}

/// Local stream parsing error type (renamed to avoid conflict with imported StreamError)
#[derive(Debug, PartialEq)]
pub enum StreamParseError<E> {
    /// Invalid UTF-8 in transport chunk
    Utf8(FromUtf8Error),
    /// Malformed SSE/NDJSON line
    Parser(NomError<String>),
    /// Underlying transport failure
    Transport(E),
}

impl<E> From<Utf8StreamError<E>> for StreamParseError<E> {
    fn from(err: Utf8StreamError<E>) -> Self {
        match err {
            Utf8StreamError::Utf8(e) => Self::Utf8(e),
            Utf8StreamError::Transport(e) => Self::Transport(e),
        }
    }
}

impl<E> From<NomError<&str>> for StreamParseError<E> {
    fn from(err: NomError<&str>) -> Self {
        Self::Parser(NomError::new(err.input.to_string(), err.code))
    }
}

impl<E> fmt::Display for StreamParseError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8(e) => write!(f, "UTF-8 error: {e}"),
            Self::Parser(e) => write!(f, "Parse error: {e}"),
            Self::Transport(e) => write!(f, "Transport error: {e}"),
        }
    }
}

impl<E> std::error::Error for StreamParseError<E> where E: fmt::Display + fmt::Debug + Send + Sync {}
