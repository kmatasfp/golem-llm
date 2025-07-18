use std::cell::{Ref, RefMut};
use std::task::Poll;

use golem_rust::wasm_rpc::Pollable;

use crate::event_source::error::EventSourceSearchError as SearchError;
use crate::event_source::error::StreamError as WebsearchStreamError;
use crate::event_source::stream::WebsearchStream;
use crate::event_source::types::WebsearchStreamEntry;
/// A trait that the session's concrete state object must implement.
pub trait SearchStreamState: 'static {
    /// If an unrecoverable error occurred during startup.
    fn failure(&self) -> &Option<SearchError>;
    /// Whether the stream has reached its logical end.
    fn is_finished(&self) -> bool;
    /// Mark the stream as finished.
    fn set_finished(&self);

    /// Immutable & mutable accessors to the underlying low-level stream.
    fn stream(&self) -> WebsearchStreamRef<'_>;
    fn stream_mut(&self) -> WebsearchStreamRefMut<'_>;
}

/// Public wrapper exported to the host.
///  * Converts low-level entries to a flat `Vec<WebsearchStreamEntry>`
///    expects `list<web-search-stream-entry>`; adapt as needed).
pub struct GuestSearchStream<T: SearchStreamState> {
    implementation: T,
}

impl<T: SearchStreamState> GuestSearchStream<T> {
    pub fn new(implementation: T) -> Self {
        Self { implementation }
    }

    /// A `Pollable` so the host can `await` readiness.
    pub fn subscribe(&self) -> Pollable {
        if let Some(stream) = self.implementation.stream().as_ref() {
            stream.subscribe()
        } else {
            golem_rust::bindings::wasi::clocks::monotonic_clock::subscribe_duration(0)
        }
    }

    pub fn state(&self) -> &T {
        &self.implementation
    }
}

pub trait HostSearchStream {
    fn get_next(&self) -> Option<Vec<WebsearchStreamEntry>>;
    /// A convenient blocking version.
    fn blocking_get_next(&self) -> Vec<WebsearchStreamEntry>;
}
impl<T: SearchStreamState> HostSearchStream for GuestSearchStream<T> {
    fn get_next(&self) -> Option<Vec<WebsearchStreamEntry>> {
        // Short-circuit if finished.
        if self.implementation.is_finished() {
            return Some(vec![]);
        }

        // Borrow the concrete stream mutably.
        let mut stream_guard = self.implementation.stream_mut();

        if let Some(stream) = stream_guard.as_mut() {
            match stream.poll_next() {
                Poll::Ready(None) => {
                    self.implementation.set_finished();
                    Some(vec![])
                }
                Poll::Ready(Some(Err(err))) => {
                    // Map low-level error => SearchError => vector
                    let err = SearchError::from(err);
                    self.implementation.set_finished();
                    Some(vec![WebsearchStreamEntry::Unknown(err.to_string())])
                }
                Poll::Ready(Some(Ok(entry))) => {
                    // A single NDJSON / SSE entry may map to 0-n public events.
                    // Here we forward it verbatim; adapt if you need to split.
                    Some(vec![entry])
                }
                Poll::Pending => None,
            }
        } else if let Some(err) = self.implementation.failure().clone() {
            self.implementation.set_finished();
            Some(vec![WebsearchStreamEntry::Unknown(err.to_string())])
        } else {
            None
        }
    }

    fn blocking_get_next(&self) -> Vec<WebsearchStreamEntry> {
        let pollable = self.subscribe();
        let mut out = Vec::new();
        loop {
            pollable.block();
            if let Some(chunk) = self.get_next() {
                out.extend(chunk);
                break out;
            }
        }
    }
}

type WebsearchStreamRef<'a> = Ref<
    'a,
    Option<
        Box<
            dyn WebsearchStream<
                    Item = WebsearchStreamEntry,
                    Error = WebsearchStreamError<reqwest::Error>,
                > + 'a,
        >,
    >,
>;
type WebsearchStreamRefMut<'a> = RefMut<
    'a,
    Option<
        Box<
            dyn WebsearchStream<
                    Item = WebsearchStreamEntry,
                    Error = WebsearchStreamError<reqwest::Error>,
                > + 'a,
        >,
    >,
>;
