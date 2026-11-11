//! Error type shared by every API call.

use crate::text::TextResponse;

/// Result alias used across the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong talking to the Peacockery Voice API.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 401: missing, wrong or revoked API key (lab keys only work on the lab host).
    #[error("unauthorized: the Peacockery Voice API key was rejected")]
    Unauthorized,
    /// 403: the key lacks the scope this route needs.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// 404: unknown upload, job or session.
    #[error("not found: {0}")]
    NotFound(String),
    /// Any other 4xx (400 validation, 409 conflict, 413, 415, 422, 426 …) with the response body.
    #[error("request rejected ({status}): {body}")]
    BadRequest { status: u16, body: String },
    /// 5xx from the API or the storage endpoint.
    #[error("server error ({status}): {body}")]
    Server { status: u16, body: String },
    /// Connection, TLS, DNS or timeout failure before an HTTP status was received.
    #[error("network error: {0}")]
    Network(String),
    /// The response could not be decoded into the expected shape.
    #[error("unexpected response: {0}")]
    Decode(String),
    /// Local file problem.
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    /// Invalid caller input caught before any request was sent.
    #[error("invalid options: {0}")]
    Config(String),
    /// A batch job ended with `status: failed`.
    #[error("transcription job {job_id} failed: {message}")]
    JobFailed {
        job_id: String,
        message: String,
        details: Option<serde_json::Value>,
    },
    /// The realtime socket closed before a `session.completed` / `session.failed` event.
    #[error("realtime connection closed before the session ended: {0}")]
    RealtimeClosed(String),
    /// Audio was sent after the server ended the session with `session.failed`
    /// (e.g. `provider_error` "Mistral realtime upgrade failed: 429"); `retryable` comes from the server.
    #[error("realtime session failed ({code}): {message}")]
    SessionFailed {
        code: String,
        message: String,
        retryable: bool,
    },
    /// The operation was cancelled through its `CancellationToken`.
    #[error("cancelled")]
    Cancelled,
    /// A wait exceeded its deadline.
    #[error("timed out: {0}")]
    Timeout(String),
    /// `/v1/text` stopped at `maxOutputTokens` (`finishReason: "length"`). The partial response is kept.
    #[error("text output was truncated at maxOutputTokens (model {})", .0.model)]
    Truncated(Box<TextResponse>),
}

impl Error {
    /// Map an HTTP status + body to the matching variant.
    pub(crate) fn from_status(status: u16, body: String) -> Self {
        match status {
            401 => Error::Unauthorized,
            403 => Error::Forbidden(body),
            404 => Error::NotFound(body),
            500..=599 => Error::Server { status, body },
            _ => Error::BadRequest { status, body },
        }
    }

    /// True when retrying the same request later can succeed (network trouble or 5xx).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Error::Network(_)
                | Error::Server { .. }
                | Error::Timeout(_)
                | Error::SessionFailed {
                    retryable: true,
                    ..
                }
        )
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        if err.is_decode() {
            return Error::Decode(err.to_string());
        }
        if let Some(status) = err.status() {
            return Error::from_status(status.as_u16(), err.to_string());
        }
        // Include the source chain: reqwest's top-level message hides the TLS/DNS cause.
        let mut message = err.to_string();
        let mut source = std::error::Error::source(&err);
        while let Some(cause) = source {
            message.push_str(": ");
            message.push_str(&cause.to_string());
            source = cause.source();
        }
        Error::Network(message)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Decode(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        assert!(matches!(
            Error::from_status(401, String::new()),
            Error::Unauthorized
        ));
        assert!(matches!(Error::from_status(403, "x".into()), Error::Forbidden(b) if b == "x"));
        assert!(matches!(
            Error::from_status(404, String::new()),
            Error::NotFound(_)
        ));
        assert!(matches!(
            Error::from_status(400, "bad".into()),
            Error::BadRequest { status: 400, body } if body == "bad"
        ));
        assert!(matches!(
            Error::from_status(409, String::new()),
            Error::BadRequest { status: 409, .. }
        ));
        assert!(matches!(
            Error::from_status(502, String::new()),
            Error::Server { status: 502, .. }
        ));
        assert!(Error::from_status(503, String::new()).is_transient());
        assert!(!Error::from_status(400, String::new()).is_transient());
    }
}
