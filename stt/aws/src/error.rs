use derive_more::From;
use hmac::digest::InvalidLength;
use http::header::InvalidHeaderValue;

#[allow(unused)]
#[derive(Debug, From)]
pub enum Error {
    #[from]
    InvalidHeader(InvalidHeaderValue),
    #[from]
    HmacSha256ErrorInvalidLength(InvalidLength),
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl std::error::Error for Error {}
