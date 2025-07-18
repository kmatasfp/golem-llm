use std::task::Poll;
use crate::event_source::stream::WebsearchStream;
use crate::event_source::{
    parser::{ is_bom, line, RawEventLine },
    utf8_stream::Utf8Stream,
    error::StreamError,
};
use crate::event_source::types::{ SearchMetadata, SearchResult, StreamEnd, WebsearchStreamEntry };

use golem_rust::bindings::wasi::io::streams::{ InputStream, StreamError as WasiStreamError };
use golem_rust::wasm_rpc::Pollable;
use log::trace;
use serde_json::from_str;

#[derive(Default, Debug)]
struct EventBuilder {
    data: String,
    is_complete: bool,
}

impl EventBuilder {
    /// ### From the HTML spec
    /// -> If the field name is `"event"`
    ///    *Ignored for web-search; we always treat the entry as JSON data.*
    /// -> If the field name is `"data"`
    ///    Append the field value to the data buffer, then append a single
    ///    `U+000A LINE FEED (LF)` character to the data buffer.
    /// -> If the field name is `"id"`
    ///    *Ignored for web-search. (No resume semantics needed here.)*
    /// -> If the field name is `"retry"`
    ///    *Ignored for web-search.*
    /// -> Otherwise
    ///    The field is ignored.
    fn add(&mut self, line: RawEventLine) {
        match line {
            RawEventLine::Field("data", val) => {
                self.data.push_str(val.unwrap_or(""));
                self.data.push('\n');
            }
            RawEventLine::Empty => {
                self.is_complete = true;
            }
            _ => {} // ignore comments, id, retry, etc.
        }
    }
    /// ### From the HTML spec
    ///
    /// 1. **(Resume not needed)** – We do not track `lastEventId` for web-search.
    /// 2. If the data buffer is an empty string, reset buffers and return `None`.
    /// 3. If the data buffer's last character is a `U+000A LINE FEED (LF)`, remove it.
    /// 4. Deserialize the buffer:
    ///    * `SearchResult` → `WebsearchStreamEntry::Result`
    ///    * `SearchMetadata` → `WebsearchStreamEntry::Metadata`
    ///    * `StreamEnd { kind: "done" }` → `WebsearchStreamEntry::Done`
    /// 5. Unknown / malformed → `WebsearchStreamEntry::Unknown(raw)`.
    /// 6. Reset internal buffers for the next event.
    fn dispatch(&mut self) -> Option<WebsearchStreamEntry> {
        if self.data.is_empty() {
            *self = Self::default();
            return None;
        }

        // Remove trailing LF.
        if let Some('\n') = self.data.chars().last() {
            self.data.pop();
        }

        let raw = core::mem::take(&mut self.data);
        self.is_complete = false;

        if let Ok(r) = from_str::<SearchResult>(&raw) {
            return Some(WebsearchStreamEntry::Result(r));
        }
        if let Ok(m) = from_str::<SearchMetadata>(&raw) {
            return Some(WebsearchStreamEntry::Metadata(m));
        }
        if let Ok(d) = from_str::<StreamEnd>(&raw) {
            if d.kind == "done" {
                return Some(WebsearchStreamEntry::Done);
            }
        }
        Some(WebsearchStreamEntry::Unknown(raw))
    }
}

/// Internal state machine.
#[derive(Debug, Clone, Copy)]
enum StreamState {
    NotStarted,
    Started,
    Terminated,
}

impl StreamState {
    fn is_started(self) -> bool {
        matches!(self, Self::Started)
    }
    fn is_terminated(self) -> bool {
        matches!(self, Self::Terminated)
    }
}

/// Public SSE stream that yields `WebsearchStreamEntry`.
pub struct SseWebsearchStream {
    stream: Utf8Stream,
    buffer: String,
    builder: EventBuilder,
    state: StreamState,
    last_event_id: Option<String>,
}

impl WebsearchStream for SseWebsearchStream {
    type Item = WebsearchStreamEntry;
    type Error = StreamError<WasiStreamError>;

    // REMOVED: new() method - not part of trait definition
    // If needed, use the create() method below instead

    fn subscribe(&self) -> Pollable {
        self.stream.subscribe()
    }

    fn poll_next(&mut self) -> Poll<Option<Result<Self::Item, Self::Error>>> {
        trace!("Polling SSE stream for next web-search entry");

        // First, drain any complete event already in `buffer`.
        if let Some(entry) = try_parse(&mut self.buffer, &mut self.builder)? {
            return Poll::Ready(Some(Ok(entry)));
        }

        if self.state.is_terminated() {
            return Poll::Ready(None);
        }

        // Otherwise read more data.
        loop {
            match self.stream.poll_next() {
                Poll::Ready(Some(Ok(chunk))) => {
                    if chunk.is_empty() {
                        continue;
                    }

                    let slice = if self.state.is_started() {
                        &chunk
                    } else {
                        self.state = StreamState::Started;
                        // Strip optional UTF-8 BOM.
                        if is_bom(chunk.chars().next().unwrap()) {
                            &chunk[1..]
                        } else {
                            &chunk
                        }
                    };

                    self.buffer.push_str(slice);

                    if let Some(entry) = try_parse(&mut self.buffer, &mut self.builder)? {
                        return Poll::Ready(Some(Ok(entry)));
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(e.into())));
                }
                Poll::Ready(None) => {
                    self.state = StreamState::Terminated;
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }

    // FIXED: Corrected method signature to match trait
    fn set_last_event_id_str(&mut self, id: String) {
        self.last_event_id = Some(id);
    }

    fn last_event_id(&self) -> &str {
        self.last_event_id.as_deref().unwrap_or("")
    }
}

impl SseWebsearchStream {
    /// Alternative constructor for creating instances without trait constraints
    pub fn create(input: InputStream) -> Self {
        Self {
            stream: Utf8Stream::new(input),
            buffer: String::new(),
            builder: EventBuilder::default(),
            state: StreamState::NotStarted,
            last_event_id: None,
        }
    }

    /// Constructor that creates a new instance from an InputStream
    pub fn new(input: InputStream) -> Self {
        Self::create(input)
    }

    /// Get the underlying pollable for subscription
    pub fn get_pollable(&self) -> Pollable {
        self.stream.subscribe()
    }

    /// Set last event ID using string slice (convenience method)
    pub fn set_last_event_id_str(&mut self, id: &str) {
        self.last_event_id = Some(id.to_string());
    }
}

fn try_parse<E>(
    buf: &mut String,
    builder: &mut EventBuilder
) -> Result<Option<WebsearchStreamEntry>, StreamError<E>> {
    if buf.is_empty() {
        return Ok(None);
    }

    loop {
        match line(buf.as_ref()) {
            Ok((rest, ln)) => {
                builder.add(ln);
                let consumed = buf.len() - rest.len();
                *buf = buf.split_off(consumed);

                if builder.is_complete {
                    return Ok(builder.dispatch());
                }
            }
            Err(nom::Err::Incomplete(_)) => {
                return Ok(None);
            }
            Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
                return Err(e.into());
            }
        }
    }
}
