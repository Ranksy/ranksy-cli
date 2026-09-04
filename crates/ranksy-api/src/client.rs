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

    async fn post(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value, ApiError> {
        self.send(reqwest::Method::POST, path, &[], Some(body)).await
    }

    async fn delete(&self, path: &str) -> Result<serde_json::Value, ApiError> {
        self.send(reqwest::Method::DELETE, path, &[], None).await
    }

    /// Generic GET against `/apps/{app}/{suffix}` with optional query params.
    /// Backs the analytics-parity commands, which need no per-endpoint typing.
    pub async fn get_app(
        &self,
        app: &str,
        suffix: &str,
        query: &[(&str, &str)],
    ) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/{suffix}"), query).await
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
    /// Track one keyword. `POST /apps/{app}/keywords` matches on the keyword
    /// TEXT server-side, so send the raw keyword — not a slug. 201 when newly
    /// tracked, 200 on an idempotent re-track; both return the follow row.
    pub async fn track_keyword(&self, app: &str, keyword: &str) -> Result<serde_json::Value, ApiError> {
        self.post(
            &format!("/apps/{app}/keywords"),
            serde_json::json!({ "keyword": keyword }),
        )
        .await
    }
    /// Untrack one keyword. `DELETE /apps/{app}/keywords/{keyword}` resolves the
    /// keyword by its SLUG, so slugify the input first: `untrack "Email Marketing"`
    /// and `untrack email-marketing` both hit `email-marketing`. The endpoint
    /// 204s with no body, so synthesize a small confirmation for output; a
    /// keyword that isn't tracked comes back as a 404 status error.
    pub async fn untrack_keyword(&self, app: &str, keyword: &str) -> Result<serde_json::Value, ApiError> {
        let slug = slugify(keyword);
        self.delete(&format!("/apps/{app}/keywords/{slug}")).await?;
        Ok(serde_json::json!({
            "object": "untracked_keyword",
            "keyword": keyword,
            "slug": slug,
        }))
    }
    pub async fn list_reviews(&self, app: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/reviews"), &[]).await
    }
    pub async fn get_installs(&self, app: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/installs"), &[]).await
    }
    pub async fn get_listing(&self, app: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/listing"), &[]).await
    }
}

/// Slugify a keyword the way the app does (`Str::slug` for ASCII): lowercase,
/// every run of non-alphanumerics becomes a single `-`, with no leading or
/// trailing dash. Idempotent on an existing slug. The API's untrack route keys
/// on the slug, so this lets a caller pass the human keyword. Edge case: a
/// keyword whose base slug collided on track carries a sha1 suffix server-side
/// — untrack it with the exact slug from `keywords list`.
fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_lowercases_and_hyphenates() {
        assert_eq!(slugify("Email Marketing"), "email-marketing");
    }

    #[test]
    fn slugify_is_idempotent_on_a_slug() {
        assert_eq!(slugify("email-marketing"), "email-marketing");
    }

    #[test]
    fn slugify_collapses_and_trims_separators() {
        assert_eq!(slugify("  SEO & Sales!!  "), "seo-sales");
    }
}
