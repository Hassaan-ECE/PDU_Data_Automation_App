use super::message::{build_payload, build_teams_payload, NotificationMessage};
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde_json::Value;
use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;
use thiserror::Error;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFailure {
    TimedOut,
    CouldNotConnect,
    RequestRejected,
    Other,
}

impl fmt::Display for TransportFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TimedOut => "request timed out",
            Self::CouldNotConnect => "connection failed",
            Self::RequestRejected => "request could not be sent",
            Self::Other => "transport failed",
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TeamsError {
    #[error("Webhook URL is empty")]
    EmptyWebhookUrl,
    #[error("Webhook URL is invalid")]
    InvalidWebhookUrl,
    #[error("Message text is empty")]
    EmptyMessage,
    #[error("Could not initialize the Teams HTTP client")]
    ClientInitialization,
    #[error("Failed to reach Teams webhook ({0})")]
    Transport(TransportFailure),
    #[error("Teams webhook HTTP {status}")]
    HttpStatus { status: reqwest::StatusCode },
    #[error("Internal error: Teams payload is missing its attachments array")]
    InvalidPayload,
}

/// Reusable blocking webhook client. One call performs one POST; there is no
/// retry loop, and redirects are disabled to keep delivery deterministic.
#[derive(Clone)]
pub struct TeamsClient {
    client: Client,
}

impl TeamsClient {
    pub fn new() -> Result<Self, TeamsError> {
        Self::with_timeout(DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Result<Self, TeamsError> {
        install_crypto_provider();
        let client = Client::builder()
            .timeout(timeout)
            .redirect(Policy::none())
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| TeamsError::ClientInitialization)?;
        Ok(Self { client })
    }

    pub fn post_message(
        &self,
        webhook_url: &str,
        message: &NotificationMessage,
    ) -> Result<(), TeamsError> {
        self.post_value(webhook_url, message.text(), &build_teams_payload(message))
    }

    pub fn post_text(&self, webhook_url: &str, text: &str) -> Result<(), TeamsError> {
        self.post_value(webhook_url, text, &build_payload(text))
    }

    fn post_value(&self, webhook_url: &str, text: &str, payload: &Value) -> Result<(), TeamsError> {
        let webhook_url = webhook_url.trim();
        if webhook_url.is_empty() {
            return Err(TeamsError::EmptyWebhookUrl);
        }
        let parsed_url = reqwest::Url::parse(webhook_url)
            .ok()
            .filter(is_allowed_webhook_url)
            .ok_or(TeamsError::InvalidWebhookUrl)?;
        if text.trim().is_empty() {
            return Err(TeamsError::EmptyMessage);
        }
        if payload
            .get("attachments")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            return Err(TeamsError::InvalidPayload);
        }

        let response = self
            .client
            .post(parsed_url)
            .json(payload)
            .send()
            .map_err(|error| TeamsError::Transport(classify_transport(&error)))?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            // Never include reqwest's error text, response body, or request URL:
            // Power Automate webhook query strings are signed credentials.
            Err(TeamsError::HttpStatus { status })
        }
    }
}

fn is_allowed_webhook_url(url: &reqwest::Url) -> bool {
    if url.scheme() == "https" {
        return true;
    }

    // Unit tests use a loopback listener to verify response/error redaction.
    // Production builds require HTTPS so the signed Workflow credential is
    // never transmitted in cleartext.
    cfg!(test)
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "::1" | "localhost"))
}

fn install_crypto_provider() {
    static PROVIDER_INSTALLED: OnceLock<()> = OnceLock::new();
    PROVIDER_INSTALLED.get_or_init(|| {
        // Reqwest's `rustls-no-provider` feature keeps this binary on the Ring
        // provider already used by the Tauri updater. A provider installed by
        // another component wins; that is also a valid process-wide setup.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn classify_transport(error: &reqwest::Error) -> TransportFailure {
    if error.is_timeout() {
        TransportFailure::TimedOut
    } else if error.is_connect() {
        TransportFailure::CouldNotConnect
    } else if error.is_request() || error.is_builder() {
        TransportFailure::RequestRejected
    } else {
        TransportFailure::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn validates_url_and_text_without_echoing_secret() {
        let client = TeamsClient::new().unwrap();
        assert_eq!(
            client.post_text("", "hello"),
            Err(TeamsError::EmptyWebhookUrl)
        );
        let error = client
            .post_text("not-a-url?sig=TOP_SECRET", "hello")
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Webhook URL is invalid");
        assert!(!error.contains("TOP_SECRET"));
        assert_eq!(
            client.post_text("http://example.invalid/hook?sig=TOP_SECRET", "hello"),
            Err(TeamsError::InvalidWebhookUrl)
        );
        assert_eq!(
            client.post_text("https://example.invalid/hook", "  "),
            Err(TeamsError::EmptyMessage)
        );
    }

    #[test]
    fn http_error_omits_signed_url_and_response_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 8192];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 36\r\nConnection: close\r\n\r\nsig=TOP_SECRET echoed by remote host",
                )
                .unwrap();
        });
        let url = format!("http://{address}/workflow?sig=TOP_SECRET");
        let error = TeamsClient::new()
            .unwrap()
            .post_text(&url, "hello")
            .unwrap_err()
            .to_string();
        server.join().unwrap();
        assert!(error.contains("400 Bad Request"));
        assert!(!error.contains("TOP_SECRET"));
        assert!(!error.contains(&url));
        assert!(!error.contains("echoed by remote"));
    }
}
