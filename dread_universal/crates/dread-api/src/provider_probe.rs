// Provider connectivity probe for registered openai-compatible endpoints.
// Author: Kimi K3.1 Pro (NVIDIA NIM), generated 2026-11-23 via /tmp/kimi_run.sh v2
// (prompt /tmp/prompt_b3b.txt, raw output /tmp/kimi_b3b_probe.rs).
// Lead review fixes before integration (see git blame):
//   1. redact_key: Kimi searched a fully lowercased string by BYTE position — Unicode
//      lowercasing can change byte length (e.g. 'İ'), desyncing offsets and panicking
//      on a non-char-boundary slice. Replaced with a length-preserving ASCII
//      case-insensitive scan. API keys are ASCII (Bearer tokens), so coverage is equal.
//   2. Added Serialize (the report crosses the HTTP API).
//   3. Collapsed an if/else whose branches were identical.

use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProbeReport {
    pub ok: bool,
    pub models: Vec<String>,
    pub latency_ms: u64,
    pub detail: String,
}

/// Case-insensitive removal of `api_key` from free-form error text.
/// ASCII-only matching is deliberate: byte offsets must stay aligned with
/// the original string, and `to_lowercase()` can change byte length for
/// non-ASCII input. Bearer/Authorization keys are always ASCII.
pub fn redact_key(text: &str, api_key: Option<&str>) -> String {
    let key = match api_key {
        Some(k) if !k.is_empty() && k.is_ascii() => k.as_bytes(),
        // Non-ASCII keys cannot occur for Authorization headers; if one ever
        // does, redact nothing rather than risk misaligned slicing.
        _ => return text.to_string(),
    };
    let bytes = text.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut i = 0usize;
    while i + key.len() <= bytes.len() {
        if bytes[i..i + key.len()].eq_ignore_ascii_case(key) {
            // Safe: matches are pure ASCII runs, so i and i+key.len() are
            // always valid char boundaries in the surrounding UTF-8.
            result.push_str(&text[last..i]);
            result.push_str("[redacted]");
            i += key.len();
            last = i;
        } else {
            i += 1;
        }
    }
    result.push_str(&text[last..]);
    result
}

fn report(ok: bool, models: Vec<String>, start: Instant, detail: String) -> ProbeReport {
    ProbeReport {
        ok,
        models,
        latency_ms: start.elapsed().as_millis() as u64,
        detail,
    }
}

fn extract_ids(value: &serde_json::Value) -> Vec<String> {
    let arr = if value.is_array() {
        value.as_array().cloned()
    } else if let Some(obj) = value.as_object() {
        obj.get("data")
            .or_else(|| obj.get("models"))
            .and_then(|v| v.as_array())
            .cloned()
    } else {
        None
    };
    arr.unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| entry.as_str().map(|s| s.to_string()))
        })
        .collect()
}

pub async fn probe_openai_compat(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    model_hint: Option<&str>,
) -> ProbeReport {
    let start = Instant::now();
    let base = base_url.trim_end_matches('/');
    let models_url = format!("{}/models", base);
    let chat_url = format!("{}/chat/completions", base);

    let mut models_req = client.get(&models_url).timeout(Duration::from_secs(20));
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        models_req = models_req.header("Authorization", format!("Bearer {}", key));
    }

    let mut models: Vec<String> = Vec::new();
    let mut models_ok = false;
    let models_detail: String;

    match models_req.send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(value) => {
                        models = extract_ids(&value);
                        models_ok = true;
                        models_detail = format!("listed {} model(s)", models.len());
                    }
                    Err(e) => {
                        models_detail = format!("models parse error: {}", e);
                    }
                }
            } else {
                models_detail = format!("models endpoint returned {}", status.as_u16());
            }
        }
        Err(e) => {
            models_detail = format!("models request error: {}", e);
        }
    }

    if models_ok {
        return report(true, models, start, redact_key(&models_detail, api_key));
    }

    tracing::warn!(detail = %redact_key(&models_detail, api_key), "models probe failed, trying chat probe");

    let model = model_hint.unwrap_or("test");
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
        "stream": false
    });

    let mut chat_req = client
        .post(&chat_url)
        .timeout(Duration::from_secs(20))
        .json(&body);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        chat_req = chat_req.header("Authorization", format!("Bearer {}", key));
    }

    match chat_req.send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                report(
                    true,
                    models,
                    start,
                    redact_key(
                        &format!(
                            "chat probe succeeded ({}); {}",
                            status.as_u16(),
                            models_detail
                        ),
                        api_key,
                    ),
                )
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                tracing::warn!(status = status.as_u16(), "chat probe rejected: bad key");
                report(
                    false,
                    models,
                    start,
                    redact_key(
                        &format!(
                            "reachable but rejected: bad key (HTTP {}); {}",
                            status.as_u16(),
                            models_detail
                        ),
                        api_key,
                    ),
                )
            } else {
                tracing::warn!(status = status.as_u16(), "chat probe failed");
                report(
                    false,
                    models,
                    start,
                    redact_key(
                        &format!(
                            "reachable but rejected (HTTP {}); {}",
                            status.as_u16(),
                            models_detail
                        ),
                        api_key,
                    ),
                )
            }
        }
        Err(e) => {
            tracing::warn!(error = %redact_key(&e.to_string(), api_key), "chat probe network error");
            report(
                false,
                models,
                start,
                redact_key(&format!("unreachable: {}", e), api_key),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::get, routing::post};
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    async fn spawn(app: Router) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{}", addr), handle)
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn models_listing_success() {
        let app = Router::new().route(
            "/v1/models",
            get(|| async { Json(serde_json::json!({"data": [{"id": "m1"}, {"id": "m2"}]})) }),
        );
        let (base, handle) = spawn(app).await;
        let url = format!("{}/v1", base);
        let report = probe_openai_compat(&client(), &url, Some("secret-key"), None).await;
        handle.abort();
        assert!(report.ok);
        assert_eq!(report.models, vec!["m1".to_string(), "m2".to_string()]);
        assert!(!report.detail.contains("secret-key"));
    }

    #[tokio::test]
    async fn models_404_then_chat_200() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                Json(serde_json::json!({"choices": [{"message": {"content": "pong"}}]}))
            }),
        );
        let (base, handle) = spawn(app).await;
        let url = format!("{}/v1", base);
        let report = probe_openai_compat(&client(), &url, None, Some("test-model")).await;
        handle.abort();
        assert!(report.ok);
        assert!(report.detail.contains("chat probe succeeded"));
    }

    #[tokio::test]
    async fn unauthorized_401() {
        let app = Router::new()
            .route(
                "/v1/models",
                get(|| async { (axum::http::StatusCode::UNAUTHORIZED, "no") }),
            )
            .route(
                "/v1/chat/completions",
                post(|| async { (axum::http::StatusCode::UNAUTHORIZED, "bad key") }),
            );
        let (base, handle) = spawn(app).await;
        let url = format!("{}/v1", base);
        let report = probe_openai_compat(&client(), &url, Some("super-secret"), None).await;
        handle.abort();
        assert!(!report.ok);
        assert!(report.detail.contains("bad key"));
        assert!(!report.detail.contains("super-secret"));
    }

    #[tokio::test]
    async fn connect_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("http://{}/v1", addr);
        let report = probe_openai_compat(&client(), &url, None, None).await;
        assert!(!report.ok);
        assert!(report.detail.contains("unreachable"));
    }

    #[test]
    fn redaction_case_insensitive() {
        let out = redact_key("error with SECRET and secret", Some("secret"));
        assert!(!out.to_lowercase().contains("secret"));
        assert!(out.contains("[redacted]"));
        assert_eq!(redact_key("unchanged", Some("")), "unchanged");
        assert_eq!(redact_key("unchanged", None), "unchanged");
    }

    #[test]
    fn redaction_survives_multibyte_text_before_the_key() {
        // Regression guard for the review fix: multibyte UTF-8 (and text whose
        // lowercase form changes byte length, like 'İ') must not desync offsets.
        let out = redact_key("ошибка İ ε→Ω key=SEKRIT9 end", Some("SEKRIT9"));
        assert_eq!(out, "ошибка İ ε→Ω key=[redacted] end");
    }
}
