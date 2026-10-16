//! Minimal Chrome DevTools Protocol client: one WebSocket to one tab, JSON-RPC
//! request/response with a pending map so several `Runtime.evaluate` calls can be
//! in flight at once (the `--concurrency` days).

use anyhow::{anyhow, bail, Context, Result};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex as AsyncMutex};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Sink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// A connected tab. Cheap to clone; clones share the connection.
#[derive(Clone)]
pub struct Tab {
    inner: Arc<Inner>,
}

struct Inner {
    sink: AsyncMutex<Sink>,
    pending: Pending,
    next_id: AtomicU64,
    reader: tokio::task::JoinHandle<()>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl Tab {
    pub async fn connect(ws_url: &str) -> Result<Self> {
        // A day's result can be several MB; lift tungstenite's default frame/message caps.
        let config = WebSocketConfig::default()
            .max_message_size(None)
            .max_frame_size(None);
        let (stream, _) = tokio_tungstenite::connect_async_with_config(ws_url, Some(config), false)
            .await
            .with_context(|| format!("cannot connect to Chrome tab at {ws_url}"))?;
        let (sink, mut read) = stream.split();
        let pending: Pending = Arc::default();

        let reader_pending = pending.clone();
        let reader = tokio::spawn(async move {
            while let Some(Ok(msg)) = read.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                    continue;
                };
                let Some(id) = value.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                let Some(tx) = reader_pending.lock().expect("pending lock").remove(&id) else {
                    continue;
                };
                let _ = tx.send(response_result(&value));
            }
            // Connection closed: fail everything still waiting.
            let waiting: Vec<_> = reader_pending
                .lock()
                .expect("pending lock")
                .drain()
                .collect();
            for (_, tx) in waiting {
                let _ = tx.send(Err("Chrome closed the connection".to_string()));
            }
        });

        Ok(Self {
            inner: Arc::new(Inner {
                sink: AsyncMutex::new(sink),
                pending,
                next_id: AtomicU64::new(0),
                reader,
            }),
        })
    }

    /// Sends one CDP command and waits for its result object.
    pub async fn send(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .expect("pending lock")
            .insert(id, tx);

        let frame = json!({ "id": id, "method": method, "params": params }).to_string();
        if let Err(e) = self
            .inner
            .sink
            .lock()
            .await
            .send(Message::text(frame))
            .await
        {
            self.inner.pending.lock().expect("pending lock").remove(&id);
            bail!("cannot send {method} to Chrome: {e}");
        }

        match rx.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(e)) => Err(anyhow!("{method} failed: {e}")),
            Err(_) => Err(anyhow!("{method}: Chrome connection dropped")),
        }
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.send("Page.navigate", json!({ "url": url })).await?;
        Ok(())
    }

    /// Evaluates an expression in the page (awaiting promises) and returns its JSON value.
    pub async fn evaluate(&self, expression: &str) -> Result<Value> {
        let result = self
            .send(
                "Runtime.evaluate",
                json!({ "expression": expression, "awaitPromise": true, "returnByValue": true }),
            )
            .await?;
        evaluation_value(&result)
    }

    pub async fn close(&self) {
        let _ = self.inner.sink.lock().await.close().await;
    }
}

/// Splits a CDP response frame into its `result` or an error string.
fn response_result(frame: &Value) -> Result<Value, String> {
    match frame.get("error") {
        Some(err) => Err(err.to_string()),
        None => Ok(frame.get("result").cloned().unwrap_or(Value::Null)),
    }
}

/// Extracts the by-value result of a `Runtime.evaluate` response, or the page exception.
fn evaluation_value(result: &Value) -> Result<Value> {
    if let Some(details) = result.get("exceptionDetails") {
        let message = details
            .pointer("/exception/description")
            .and_then(Value::as_str)
            .or_else(|| details.get("text").and_then(Value::as_str))
            .unwrap_or("evaluation failed");
        bail!("{message}");
    }

    Ok(result
        .pointer("/result/value")
        .cloned()
        .unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_returns_value() {
        let r = json!({ "result": { "type": "object", "value": { "href": "https://x" } } });
        assert_eq!(
            evaluation_value(&r).unwrap(),
            json!({ "href": "https://x" })
        );
    }

    #[test]
    fn evaluation_without_value_is_null() {
        let r = json!({ "result": { "type": "undefined" } });
        assert_eq!(evaluation_value(&r).unwrap(), Value::Null);
    }

    #[test]
    fn evaluation_exception_is_an_error() {
        let r = json!({ "result": {}, "exceptionDetails": { "text": "Uncaught", "exception": { "description": "ReferenceError: x" } } });
        assert_eq!(
            evaluation_value(&r).unwrap_err().to_string(),
            "ReferenceError: x"
        );
    }

    /// A fake Chrome tab: answers `Runtime.evaluate` out of order to prove responses are
    /// matched by id, and fails one with an exception.
    #[tokio::test]
    async fn multiplexes_concurrent_evaluations_over_one_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut requests = Vec::new();
            while requests.len() < 3 {
                if let Some(Ok(Message::Text(t))) = ws.next().await {
                    requests.push(serde_json::from_str::<Value>(t.as_str()).unwrap());
                }
            }
            // Reply in reverse order; an unrelated event frame first.
            ws.send(Message::text(
                json!({ "method": "Page.loadEventFired", "params": {} }).to_string(),
            ))
            .await
            .unwrap();
            for req in requests.iter().rev() {
                let expr = req["params"]["expression"].as_str().unwrap();
                let result = if expr == "boom" {
                    json!({ "result": {}, "exceptionDetails": { "text": "x", "exception": { "description": "Error: boom" } } })
                } else {
                    json!({ "result": { "type": "string", "value": format!("echo:{expr}") } })
                };
                ws.send(Message::text(
                    json!({ "id": req["id"], "result": result }).to_string(),
                ))
                .await
                .unwrap();
            }
            // Keep the socket open until the client is done.
            let _ = ws.next().await;
        });

        let tab = Tab::connect(&url).await.unwrap();
        let (a, b, c) = tokio::join!(
            tab.evaluate("one"),
            tab.evaluate("boom"),
            tab.evaluate("three")
        );
        assert_eq!(a.unwrap(), json!("echo:one"));
        assert_eq!(b.unwrap_err().to_string(), "Error: boom");
        assert_eq!(c.unwrap(), json!("echo:three"));
        tab.close().await;
    }

    #[tokio::test]
    async fn pending_calls_fail_when_chrome_goes_away() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _ = ws.next().await;
            // Drop the socket without answering.
        });

        let tab = Tab::connect(&url).await.unwrap();
        let err = tab
            .evaluate("never answered")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("closed") || err.contains("dropped"), "{err}");
    }

    #[test]
    fn response_error_is_surfaced() {
        let frame = json!({ "id": 1, "error": { "code": -32000, "message": "nope" } });
        assert!(response_result(&frame).unwrap_err().contains("nope"));
        assert_eq!(
            response_result(&json!({ "id": 1, "result": { "a": 1 } })).unwrap(),
            json!({ "a": 1 })
        );
    }
}
