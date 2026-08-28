use std::time::Duration;

const MAX_RETRIES: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api returned {code}: {body}")]
    Status { code: u16, body: String },
    #[error("client build error: {0}")]
    Build(String),
    #[error("{0}")]
    NotImplemented(&'static str),
}

pub struct ClientConfig {
    pub api_key: String,
    pub base_url: String,
}

pub struct RanksyClient {
    http: reqwest::Client,
    base_url: String,
}

impl RanksyClient {
    pub fn new(cfg: ClientConfig) -> Result<Self, ApiError> {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
        let mut headers = HeaderMap::new();
        let value = HeaderValue::from_str(&format!("Bearer {}", cfg.api_key))
            .map_err(|e| ApiError::Build(e.to_string()))?;
        headers.insert(AUTHORIZATION, value);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent(concat!("ranksy-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http, base_url: cfg.base_url.trim_end_matches('/').to_string() })
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, ApiError> {
        self.send(reqwest::Method::GET, path, query, None).await
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        let mut attempt = 0;
        loop {
            let mut req = self.http.request(method.clone(), &url).query(query);
            if let Some(b) = &body {
                req = req.json(b);
            }
            let resp = req.send().await?;
            let status = resp.status();
            if status.is_success() {
                return Ok(resp.json().await.unwrap_or(serde_json::Value::Null));
            }
            let retriable = status.as_u16() == 429 || status.is_server_error();
            if retriable && attempt < MAX_RETRIES {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(200 * 2u64.pow(attempt))).await;
                continue;
            }
            let code = status.as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Status { code, body });
        }
    }

    /// `whoami`: the API has no dedicated endpoint, so this lists the apps
    /// accessible with the key — proving the key works.
    pub async fn whoami(&self) -> Result<serde_json::Value, ApiError> {
        self.get("/apps", &[]).await
    }
    pub async fn list_apps(&self) -> Result<serde_json::Value, ApiError> {
        self.get("/apps", &[]).await
    }
    pub async fn get_rankings(&self, app: &str, keyword: Option<&str>) -> Result<serde_json::Value, ApiError> {
        if keyword.is_some() {
            return Err(ApiError::NotImplemented(
                "the Ranksy API has no keyword filter on rankings; use `ranksy keywords list` instead",
            ));
        }
        self.get(&format!("/apps/{app}/rankings"), &[]).await
    }
    pub async fn list_keywords(&self, app: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/keywords"), &[]).await
    }
    pub async fn track_keyword(&self, _app: &str, _keyword: &str) -> Result<serde_json::Value, ApiError> {
        Err(ApiError::NotImplemented("keyword tracking has no API v1 endpoint yet"))
    }
    pub async fn untrack_keyword(&self, _app: &str, _keyword: &str) -> Result<serde_json::Value, ApiError> {
        Err(ApiError::NotImplemented("keyword untracking has no API v1 endpoint yet"))
    }
    pub async fn list_reviews(&self, app: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/reviews"), &[]).await
    }
    pub async fn get_installs(&self, app: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/installs"), &[]).await
    }
    pub async fn get_listing(&self, _app: &str) -> Result<serde_json::Value, ApiError> {
        Err(ApiError::NotImplemented("listing lookup has no API v1 endpoint yet"))
    }
}
