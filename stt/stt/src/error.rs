use crate::{client, golem::stt::types::SttError};

use derive_more::From;

#[allow(unused)]
#[derive(Debug, From)]
pub enum Error {
    #[from]
    Client(client::Error),

    APIBadRequest {
        provider_error: String,
    },
    APIUnauthorized {
        provider_error: String,
    },
    APIForbidden {
        provider_error: String,
    },
    APIAccessDenied {
        provider_error: String,
    },
    APINotFound {
        provider_error: String,
    },
    APIConflict {
        provider_error: String,
    },
    APIUnprocessableEntity {
        provider_error: String,
    },
    APIRateLimit {
        provider_error: String,
    },
    #[allow(clippy::enum_variant_names)]
    APIInternalServerError {
        provider_error: String,
    },
    APIUnknown {
        provider_error: String,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[allow(unused)]
#[derive(Debug, PartialEq)]
pub struct ApiError {
    pub provider_error: String,
}

impl From<Error> for SttError {
    fn from(error: Error) -> Self {
        match error {
            Error::APIBadRequest { provider_error } => SttError::InvalidAudio(provider_error),
            Error::APIUnauthorized { provider_error } => SttError::AccessDenied(provider_error),
            Error::APIForbidden { provider_error } => SttError::Unauthorized(provider_error),
            Error::APIAccessDenied { provider_error } => SttError::AccessDenied(provider_error),
            Error::APINotFound { provider_error } => SttError::UnsupportedOperation(provider_error),
            Error::APIConflict { provider_error } => SttError::ServiceUnavailable(provider_error),
            Error::APIUnprocessableEntity { provider_error } => {
                SttError::ServiceUnavailable(provider_error)
            }
            Error::APIRateLimit { provider_error } => SttError::RateLimited(provider_error),
            Error::APIInternalServerError { provider_error } => {
                SttError::ServiceUnavailable(provider_error)
            }
            Error::APIUnknown { provider_error } => SttError::InternalError(provider_error),
            Error::Client(error) => SttError::InternalError(format!("Internal error: {error}")),
        }
    }
}
