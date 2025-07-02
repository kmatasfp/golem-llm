use golem_rust::bindings::wasi::io::streams::{ InputStream, StreamError };
use golem_rust::wasm_rpc::Pollable;
use std::string::FromUtf8Error;
use std::task::Poll;

/// Read an `InputStream` as valid UTF-8 chunks.
///
/// The stream yields `Poll::Ready(Some(Ok(String)))` each time a complete UTF-8
/// sequence is available. On end-of-file it yields `None`.
pub struct Utf8Stream {
    subscription: Pollable,
    stream: InputStream,
    buffer: Vec<u8>,
    terminated: bool,
}

impl Utf8Stream {
    pub const CHUNK_SIZE: u64 = 1024;

    pub fn new(stream: InputStream) -> Self {
        Self {
            subscription: stream.subscribe(),
            stream,
            buffer: Vec::new(),
            terminated: false,
        }
    }

    /// Poll for the next UTF-8 chunk.
    pub fn poll_next(&mut self) -> Poll<Option<Result<String, Utf8StreamError<StreamError>>>> {
        if !self.terminated && self.subscription.ready() {
            match self.stream.read(Self::CHUNK_SIZE) {
                Ok(bytes) => {
                    self.buffer.extend_from_slice(bytes.as_ref());
                    let bytes = core::mem::take(&mut self.buffer);
                    match String::from_utf8(bytes) {
                        Ok(s) => Poll::Ready(Some(Ok(s))),
                        Err(e) => {
                            // keep incomplete UTF-8 sequence in buffer
                            let valid = e.utf8_error().valid_up_to();
                            let mut bytes = e.into_bytes();
                            let rem = bytes.split_off(valid);
                            self.buffer = rem;
                            // SAFETY: first `valid` bytes form valid UTF-8
                            Poll::Ready(Some(Ok(unsafe { String::from_utf8_unchecked(bytes) })))
                        }
                    }
                }
                Err(StreamError::Closed) => {
                    self.terminated = true;
                    if self.buffer.is_empty() {
                        Poll::Ready(None)
                    } else {
                        Poll::Ready(
                            Some(
                                String::from_utf8(core::mem::take(&mut self.buffer)).map_err(
                                    Utf8StreamError::Utf8
                                )
                            )
                        )
                    }
                }
                Err(e) => Poll::Ready(Some(Err(Utf8StreamError::Transport(e)))),
            }
        } else {
            Poll::Pending
        }
    }

    /// Expose the underlying pollable so callers can `await` readiness.
    pub fn subscribe(&self) -> Pollable {
        self.stream.subscribe()
    }
}

#[derive(Debug, PartialEq)]
pub enum Utf8StreamError<E> {
    Utf8(FromUtf8Error),
    Transport(E),
}

impl<E> From<FromUtf8Error> for Utf8StreamError<E> {
    fn from(e: FromUtf8Error) -> Self {
        Self::Utf8(e)
    }
}
