use std::time::Duration;

const MAX_RETRIES: u32 = 3;
/// Bulk uploads validate tens of thousands of rows server-side; the default would cut them off.
const BULK_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api returned {code}: {body}")]
    Status { code: u16, body: String },
    #[error("client build error: {0}")]
    Build(String),
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
        Ok(Self {
            http,
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
        })
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, ApiError> {
        self.send(reqwest::Method::GET, path, query, None, None)
            .await
    }

    async fn post(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ApiError> {
        self.send(reqwest::Method::POST, path, &[], Some(&body), None)
            .await
    }

    async fn delete(&self, path: &str) -> Result<serde_json::Value, ApiError> {
        self.send(reqwest::Method::DELETE, path, &[], None, None)
            .await
    }

    /// POST an arbitrary JSON body to `path` (relative to the base URL) with the usual
    /// 429/5xx retries and a long timeout, for bulk endpoints such as the ads sync whose
    /// requests carry tens of thousands of rows.
    pub async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ApiError> {
        self.send(
            reqwest::Method::POST,
            path,
            &[],
            Some(body),
            Some(BULK_TIMEOUT),
        )
        .await
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
        body: Option<&serde_json::Value>,
        timeout: Option<Duration>,
    ) -> Result<serde_json::Value, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        let mut attempt = 0;
        loop {
            let mut req = self.http.request(method.clone(), &url).query(query);
            if let Some(b) = body {
                req = req.json(b);
            }
            if let Some(t) = timeout {
                req = req.timeout(t);
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
    /// One page of `GET /apps`, for callers that need every app (`ranksy ads connect`).
    pub async fn list_apps_page(
        &self,
        page: u32,
        per_page: u32,
    ) -> Result<serde_json::Value, ApiError> {
        let (page, per_page) = (page.to_string(), per_page.to_string());
        self.get(
            "/apps",
            &[("page", page.as_str()), ("per_page", per_page.as_str())],
        )
        .await
    }
    /// Current ranking snapshot for the app. With a keyword, narrows to that one
    /// keyword via the by-keyword endpoint (the rankings list has no query-param
    /// filter, but `rankings/by-keyword/{slug}` is the same keyword branch scoped
    /// to one term).
    pub async fn get_rankings(
        &self,
        app: &str,
        keyword: Option<&str>,
    ) -> Result<serde_json::Value, ApiError> {
        if let Some(kw) = keyword {
            return self.rankings_by_keyword(app, kw, None).await;
        }
        self.get(&format!("/apps/{app}/rankings"), &[]).await
    }
    /// Current scraped rank rows for ONE keyword. The endpoint keys on the slug
    /// (case-insensitive), so slugify the argument — `"Email Marketing"` and
    /// `email-marketing` both resolve. 404 when the app doesn't track/rank it.
    pub async fn rankings_by_keyword(
        &self,
        app: &str,
        keyword: &str,
        limit: Option<i64>,
    ) -> Result<serde_json::Value, ApiError> {
        let slug = slugify(keyword);
        let limit = limit.map(|l| l.to_string());
        let query: Vec<(&str, &str)> = match &limit {
            Some(l) => vec![("limit", l.as_str())],
            None => vec![],
        };
        self.get(&format!("/apps/{app}/rankings/by-keyword/{slug}"), &query)
            .await
    }
    pub async fn list_keywords(&self, app: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/apps/{app}/keywords"), &[]).await
    }
    /// Track one keyword. `POST /apps/{app}/keywords` matches on the keyword
    /// TEXT server-side, so send the raw keyword — not a slug. 201 when newly
    /// tracked, 200 on an idempotent re-track; both return the follow row.
    pub async fn track_keyword(
        &self,
        app: &str,
        keyword: &str,
    ) -> Result<serde_json::Value, ApiError> {
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
    pub async fn untrack_keyword(
        &self,
        app: &str,
        keyword: &str,
    ) -> Result<serde_json::Value, ApiError> {
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
    use super::{slugify, ApiError, ClientConfig, RanksyClient};
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> RanksyClient {
        RanksyClient::new(ClientConfig {
            api_key: "rk_test_x".into(),
            base_url: format!("{}/api/v1/", server.uri()),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn post_json_sends_the_body_with_auth_and_retries_5xx() {
        let server = MockServer::start().await;
        let body = serde_json::json!({ "campaigns": [], "days": [{ "date": "2026-10-15" }] });
        Mock::given(method("POST"))
            .and(path("/api/v1/apps/01A/shopify-ads/sync"))
            .and(header("authorization", "Bearer rk_test_x"))
            .and(body_json(&body))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/apps/01A/shopify-ads/sync"))
            .and(body_json(&body))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": { "days": { "stored": 1 } } })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let res = client(&server)
            .post_json("/apps/01A/shopify-ads/sync", &body)
            .await
            .unwrap();
        assert_eq!(res["data"]["days"]["stored"], 1);
    }

    #[tokio::test]
    async fn post_json_surfaces_forbidden_without_retrying() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string("missing scope"))
            .expect(1)
            .mount(&server)
            .await;

        let err = client(&server)
            .post_json("/apps/01A/shopify-ads/sync", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, ApiError::Status { code: 403, ref body } if body == "missing scope"));
    }

    #[tokio::test]
    async fn list_apps_page_passes_paging_params() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/apps"))
            .and(query_param("page", "2"))
            .and(query_param("per_page", "100"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })),
            )
            .expect(1)
            .mount(&server)
            .await;

        client(&server).list_apps_page(2, 100).await.unwrap();
    }

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
