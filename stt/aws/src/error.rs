use derive_more::From;
use golem_stt::client;
use hmac::digest::InvalidLength;
use http::header::InvalidHeaderValue;

#[allow(unused)]
#[derive(Debug, From)]
pub enum Error {
    #[from]
    InvalidHeader(InvalidHeaderValue),
    #[from]
    HmacSha256ErrorInvalidLength(InvalidLength),
    #[from]
    HttpClient(client::Error),
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl std::error::Error for Error {}
