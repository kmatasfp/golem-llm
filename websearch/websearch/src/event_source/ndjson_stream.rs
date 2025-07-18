use super::stream::WebsearchStream;
use super::types::WebsearchStreamEntry;
use crate::event_source::utf8_stream::Utf8Stream;
use crate::event_source::StreamError as NdJsonStreamError;
// use crate_golem::websearch::websearch::SearchError;
use golem_rust::bindings::wasi::io::streams::{InputStream, StreamError};
use golem_rust::wasm_rpc::Pollable;
use log::{debug, error, trace, warn};
use serde_json::Value;
use std::task::Poll;

/// Represents the state of the NDJSON web search stream.
#[derive(Debug, Clone, Copy)]
pub enum NdJsonStreamState {
    NotStarted,
    Started,
    Terminated,
}

impl NdJsonStreamState {
    fn is_terminated(self) -> bool {
        matches!(self, Self::Terminated)
    }
}

/// Stream of newline-delimited JSON (NDJSON) web search results.
pub struct NdJsonWebsearchStream {
    stream: Utf8Stream,
    buffer: String,
    state: NdJsonStreamState,
    last_event_id: String,
    results_count: usize,
}

impl WebsearchStream for NdJsonWebsearchStream {
    type Item = WebsearchStreamEntry;
    type Error = NdJsonStreamError<StreamError>;
    fn set_last_event_id_str(&mut self, id: String) {
        self.last_event_id = id;
    }

    fn last_event_id(&self) -> &str {
        &self.last_event_id
    }

    fn subscribe(&self) -> Pollable {
        self.stream.subscribe()
    }

    fn poll_next(&mut self) -> Poll<Option<Result<Self::Item, Self::Error>>> {
        trace!("Polling for next NDJSON web search event");

        if let Some(entry) = try_parse_search_line(self)? {
            return Poll::Ready(Some(Ok(entry)));
        }

        if self.state.is_terminated() {
            return Poll::Ready(None);
        }

        loop {
            match self.stream.poll_next() {
                Poll::Ready(Some(Ok(chunk))) => {
                    if chunk.is_empty() {
                        continue;
                    }

                    self.state = NdJsonStreamState::Started;
                    self.buffer.push_str(&chunk);

                    if let Some(entry) = try_parse_search_line(self)? {
                        return Poll::Ready(Some(Ok(entry)));
                    }
                }
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(Some(Err(err.into())));
                }
                Poll::Ready(None) => {
                    self.state = NdJsonStreamState::Terminated;

                    if !self.buffer.trim().is_empty() {
                        let leftover = std::mem::take(&mut self.buffer);
                        warn!("Unparsed leftover buffer: {}", leftover.trim());

                        if let Ok(entry) = parse_json_to_search_entry(leftover.trim()) {
                            return Poll::Ready(Some(Ok(entry)));
                        }
                    }

                    debug!("Stream completed. Total results: {}", self.results_count);
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

impl NdJsonWebsearchStream {
    /// Constructor that creates a new instance from an InputStream
    pub fn new(stream: InputStream) -> Self {
        Self {
            stream: Utf8Stream::new(stream),
            buffer: String::new(),
            state: NdJsonStreamState::NotStarted,
            last_event_id: String::new(),
            results_count: 0,
        }
    }

    /// Alternative constructor name for consistency
    pub fn create(stream: InputStream) -> Self {
        Self::new(stream)
    }

    /// Total number of parsed `result` entries.
    pub fn results_count(&self) -> usize {
        self.results_count
    }

    /// Whether the stream has received any data.
    pub fn is_started(&self) -> bool {
        matches!(self.state, NdJsonStreamState::Started)
    }

    /// Whether the stream has ended.
    pub fn is_terminated(&self) -> bool {
        self.state.is_terminated()
    }
}

/// Parses one complete line from the stream buffer (if any).
fn try_parse_search_line(
    stream: &mut NdJsonWebsearchStream,
) -> Result<Option<WebsearchStreamEntry>, NdJsonStreamError<StreamError>> {
    if let Some(pos) = stream.buffer.find('\n') {
        let line = stream
            .buffer
            .drain(..=pos)
            .collect::<String>()
            .trim()
            .to_string();

        if line.is_empty() {
            return Ok(None);
        }

        trace!("Parsing NDJSON line: {line}");

        match parse_json_to_search_entry(&line) {
            Ok(entry) => {
                if matches!(entry, WebsearchStreamEntry::Result(_)) {
                    stream.results_count += 1;
                    debug!("Parsed result #{}", stream.results_count);
                }
                Ok(Some(entry))
            }
            Err(err) => {
                error!("Failed to parse line: {line:?} ({err})");
                Ok(Some(WebsearchStreamEntry::Unknown(line)))
            }
        }
    } else {
        Ok(None)
    }
}

/// Deserializes a JSON line into a typed `WebsearchStreamEntry`.
fn parse_json_to_search_entry(json: &str) -> Result<WebsearchStreamEntry, serde_json::Error> {
    let value: Value = serde_json::from_str(json)?;
    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("");

    match kind {
        "result" => Ok(WebsearchStreamEntry::Result(serde_json::from_str(json)?)),
        "meta" => Ok(WebsearchStreamEntry::Metadata(serde_json::from_str(json)?)),
        "done" => Ok(WebsearchStreamEntry::Done),
        _ => Ok(WebsearchStreamEntry::Unknown(json.to_string())),
    }
}
