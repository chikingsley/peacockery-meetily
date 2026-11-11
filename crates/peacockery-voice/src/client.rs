//! HTTP client: configuration, default headers, status mapping, `/health`.

use std::fmt;
use std::sync::{Arc, Once};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{Error, Result};

/// The lab host. Scribe v2 and the Superwhisper routes only exist here, and `pv_lab_` keys only work here.
pub const DEFAULT_BASE_URL: &str = "https://voice-lab.peacockery.studio";

const HEADER_APP_ID: &str = "x-peacockery-app-id";
const HEADER_APP_VERSION: &str = "x-peacockery-app-version";
const HEADER_PLATFORM: &str = "x-peacockery-platform";
const HEADER_INSTALLATION_ID: &str = "x-peacockery-installation-id";
pub(crate) const HEADER_CLIENT_SESSION_ID: &str = "x-peacockery-client-session-id";

/// Per-request timeout for ordinary JSON calls.
pub(crate) const JSON_TIMEOUT: Duration = Duration::from_secs(30);

/// Connection settings. `Debug` masks the key.
#[derive(Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// `https://host` without a trailing path.
    pub base_url: String,
    /// `pv_lab_…` key, sent as `Authorization: Bearer`.
    pub api_key: String,
    /// `X-Peacockery-App-ID`.
    pub app_id: String,
    /// `X-Peacockery-App-Version`.
    pub app_version: String,
    /// `X-Peacockery-Platform`, e.g. `windows`.
    pub platform: String,
    /// `X-Peacockery-Installation-ID`: a uuid persisted by the app.
    pub installation_id: Option<String>,
}

impl ClientConfig {
    /// Config with this crate's app identity (`peacockery-meetily`, platform of the build target).
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            app_id: "peacockery-meetily".into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            platform: std::env::consts::OS.into(),
            installation_id: None,
        }
    }
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &mask_key(&self.api_key))
            .field("app_id", &self.app_id)
            .field("app_version", &self.app_version)
            .field("platform", &self.platform)
            .field("installation_id", &self.installation_id)
            .finish()
    }
}

/// `pv_lab_1234…cdef` style mask for display; never reveals more than 4 trailing characters.
pub fn mask_key(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        return String::new();
    }
    let prefix_len = if key.starts_with("pv_lab_") || key.starts_with("pv_live_") {
        key.find('_')
            .and_then(|i| key[i + 1..].find('_').map(|j| i + j + 2))
            .unwrap_or(0)
    } else {
        0
    };
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if key.chars().count() <= prefix_len + 8 {
        return format!("{}…", &key[..prefix_len]);
    }
    format!("{}…{}", &key[..prefix_len], tail)
}

/// `GET /health`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Health {
    pub ok: bool,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub provider_policy: Option<String>,
}

/// Async Peacockery Voice client. Cheap to clone.
#[derive(Clone)]
pub struct VoiceClient {
    inner: Arc<Inner>,
}

struct Inner {
    config: ClientConfig,
    base: Url,
    /// API calls: carries the bearer and `X-Peacockery-*` headers.
    api: reqwest::Client,
    /// Signed storage URLs: must never carry the API bearer (SigV4 rejects a second auth scheme).
    transfer: reqwest::Client,
}

impl fmt::Debug for VoiceClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceClient")
            .field("config", &self.inner.config)
            .finish()
    }
}

/// rustls needs exactly one process-wide crypto provider; this crate uses ring (cross-compiles for msvc).
pub(crate) fn ensure_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // Err means another component already installed a provider, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl VoiceClient {
    /// Build a client. Fails on an unparseable base URL or a key/header value that is not valid ASCII.
    pub fn new(config: ClientConfig) -> Result<Self> {
        ensure_crypto_provider();
        let mut base = Url::parse(config.base_url.trim())
            .map_err(|e| Error::Config(format!("base_url {:?}: {e}", config.base_url)))?;
        if !matches!(base.scheme(), "https" | "http") {
            return Err(Error::Config(format!(
                "base_url must be http(s): {}",
                config.base_url
            )));
        }
        base.set_path("/");
        base.set_query(None);
        if config.api_key.trim().is_empty() {
            return Err(Error::Config("api_key is empty".into()));
        }

        let mut headers = HeaderMap::new();
        for (name, value) in identity_headers(&config)? {
            headers.insert(name, value);
        }
        let api = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::Config(format!("http client: {e}")))?;
        let transfer = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| Error::Config(format!("http client: {e}")))?;
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                base,
                api,
                transfer,
            }),
        })
    }

    /// The configuration this client was built from.
    pub fn config(&self) -> &ClientConfig {
        &self.inner.config
    }

    /// `GET /health` (needs `models:read`).
    pub async fn health(&self) -> Result<Health> {
        self.get_json("health").await
    }

    pub(crate) fn url(&self, path: &str) -> Result<Url> {
        self.inner
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|e| Error::Config(format!("url {path}: {e}")))
    }

    /// `wss://` (or `ws://`) URL for a path on the API host.
    pub(crate) fn ws_url(&self, path: &str) -> Result<Url> {
        let mut url = self.url(path)?;
        let scheme = if url.scheme() == "http" { "ws" } else { "wss" };
        url.set_scheme(scheme)
            .map_err(|_| Error::Config("cannot build websocket url".into()))?;
        Ok(url)
    }

    /// Bearer + identity headers, for requests not built by the API reqwest client (WebSocket upgrade).
    pub(crate) fn auth_headers(&self) -> Result<Vec<(HeaderName, HeaderValue)>> {
        identity_headers(&self.inner.config)
    }

    pub(crate) fn api(&self) -> &reqwest::Client {
        &self.inner.api
    }

    pub(crate) fn transfer(&self) -> &reqwest::Client {
        &self.inner.transfer
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .api()
            .get(self.url(path)?)
            .timeout(JSON_TIMEOUT)
            .send()
            .await?;
        decode(response).await
    }

    pub(crate) async fn post_json<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let response = self
            .api()
            .post(self.url(path)?)
            .timeout(JSON_TIMEOUT)
            .json(body)
            .send()
            .await?;
        decode(response).await
    }
}

fn identity_headers(config: &ClientConfig) -> Result<Vec<(HeaderName, HeaderValue)>> {
    let value = |field: &str, v: &str| {
        HeaderValue::from_str(v.trim())
            .map_err(|_| Error::Config(format!("{field} is not a valid header value")))
    };
    let mut auth = value("api_key", &format!("Bearer {}", config.api_key.trim()))?;
    auth.set_sensitive(true);
    let mut headers = vec![(AUTHORIZATION, auth)];
    let optional = [
        (HEADER_APP_ID, "app_id", Some(config.app_id.as_str())),
        (
            HEADER_APP_VERSION,
            "app_version",
            Some(config.app_version.as_str()),
        ),
        (HEADER_PLATFORM, "platform", Some(config.platform.as_str())),
        (
            HEADER_INSTALLATION_ID,
            "installation_id",
            config.installation_id.as_deref(),
        ),
    ];
    for (name, field, v) in optional {
        if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
            headers.push((HeaderName::from_static(name), value(field, v)?));
        }
    }
    Ok(headers)
}

/// Turn a non-2xx response into the matching [`Error`], keeping the body.
pub(crate) async fn check(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(Error::from_status(status.as_u16(), body))
}

pub(crate) async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let response = check(response).await?;
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        let preview: String = String::from_utf8_lossy(&bytes).chars().take(300).collect();
        Error::Decode(format!("{e}; body starts {preview:?}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_keys() {
        assert_eq!(mask_key("pv_lab_0123456789abcdef0123"), "pv_lab_…0123");
        assert_eq!(mask_key("pv_lab_short"), "pv_lab_…");
        assert_eq!(mask_key("abcdefghijklmnop"), "…mnop");
        assert_eq!(mask_key(""), "");
        let cfg = ClientConfig::new(DEFAULT_BASE_URL, "pv_lab_0123456789abcdef0123secret");
        assert!(!format!("{cfg:?}").contains("secret"));
    }

    #[test]
    fn builds_urls_and_headers() {
        let mut cfg = ClientConfig::new(
            "https://voice-lab.peacockery.studio/some/path?x=1",
            "pv_lab_x",
        );
        cfg.installation_id = Some("1b4e28ba-2fa1-11d2-883f-0016d3cca427".into());
        let client = VoiceClient::new(cfg).unwrap();
        assert_eq!(
            client.url("/v1/models").unwrap().as_str(),
            "https://voice-lab.peacockery.studio/v1/models"
        );
        assert_eq!(
            client.ws_url("v1/realtime").unwrap().as_str(),
            "wss://voice-lab.peacockery.studio/v1/realtime"
        );
        let headers = client.auth_headers().unwrap();
        let names: Vec<_> = headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "authorization",
                "x-peacockery-app-id",
                "x-peacockery-app-version",
                "x-peacockery-platform",
                "x-peacockery-installation-id"
            ]
        );
        assert_eq!(headers[0].1, "Bearer pv_lab_x");
        assert!(headers[0].1.is_sensitive());
    }

    #[test]
    fn rejects_bad_config() {
        assert!(matches!(
            VoiceClient::new(ClientConfig::new("ftp://x", "k")),
            Err(Error::Config(_))
        ));
        assert!(matches!(
            VoiceClient::new(ClientConfig::new(DEFAULT_BASE_URL, " ")),
            Err(Error::Config(_))
        ));
        assert!(matches!(
            VoiceClient::new(ClientConfig::new(DEFAULT_BASE_URL, "bad\nkey")),
            Err(Error::Config(_))
        ));
    }
}
