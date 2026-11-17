use axum::{extract::State, http::StatusCode, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{env, sync::Arc, time::Duration};

#[derive(Clone)]
struct AppState {
    http: reqwest::Client,
    llama_url: String,
    comfy_url: String,
    model: String,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    inference_backend: &'static str,
    inference_url: String,
}

#[derive(Deserialize)]
struct ChatRequest {
    messages: Vec<Value>,
    #[serde(default)]
    model: Option<String>,
    #[serde(flatten)]
    options: std::collections::HashMap<String, Value>,
}

async fn health(State(s): State<Arc<AppState>>) -> Json<Health> {
    Json(Health { status: "ok", inference_backend: "llama.cpp", inference_url: s.llama_url.clone() })
}

async fn services(State(s): State<Arc<AppState>>) -> Json<Value> {
    let checks = vec![
        ("llama_cpp", format!("{}/health", s.llama_url)),
        ("comfyui", format!("{}/system_stats", s.comfy_url)),
    ];
    let results = futures_check(s.http.clone(), checks).await;
    Json(json!({"services": results, "gpu_authority": "external-python-supervisor"}))
}

async fn futures_check(client: reqwest::Client, checks: Vec<(&'static str, String)>) -> Vec<Value> {
    let mut out = Vec::new();
    for (id, url) in checks {
        let result = match client.get(&url).send().await {
            Ok(r) => json!({"id": id, "url": url, "status": if r.status().is_success() {"ONLINE"} else {"DEGRADED"}, "http_status": r.status().as_u16()}),
            Err(e) => json!({"id": id, "url": url, "status": "OFFLINE", "detail": e.to_string()}),
        };
        out.push(result);
    }
    out
}

async fn models(State(s): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    let url = format!("{}/v1/models", s.llama_url);
    let response = s.http.get(url).send().await.map_err(upstream)?;
    let status = response.status();
    let body = response.json::<Value>().await.map_err(upstream)?;
    if !status.is_success() { return Err((StatusCode::BAD_GATEWAY, body.to_string())); }
    Ok(Json(body))
}

async fn chat(State(s): State<Arc<AppState>>, Json(mut request): Json<ChatRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    if request.messages.is_empty() { return Err((StatusCode::BAD_REQUEST, "messages must not be empty".into())); }
    let mut payload = json!({"messages": request.messages, "model": request.model.take().unwrap_or(s.model.clone()), "stream": false});
    if let Some(map) = payload.as_object_mut() {
        for (k,v) in request.options { if k != "messages" && k != "model" && k != "stream" { map.insert(k,v); } }
    }
    let response = s.http.post(format!("{}/v1/chat/completions", s.llama_url)).json(&payload).send().await.map_err(upstream)?;
    let status = response.status();
    let body = response.json::<Value>().await.map_err(upstream)?;
    if !status.is_success() { return Err((StatusCode::BAD_GATEWAY, body.to_string())); }
    Ok(Json(body))
}

async fn comfy_status(State(s): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    let response = s.http.get(format!("{}/system_stats", s.comfy_url)).send().await.map_err(upstream)?;
    let status = response.status();
    let body = response.json::<Value>().await.map_err(upstream)?;
    if !status.is_success() { return Err((StatusCode::BAD_GATEWAY, body.to_string())); }
    Ok(Json(body))
}

fn upstream(e: reqwest::Error) -> (StatusCode, String) { (StatusCode::BAD_GATEWAY, format!("upstream request failed: {e}")) }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bind = env::var("DREAD_BIND").unwrap_or_else(|_| "127.0.0.1:8765".into());
    let llama_url = env::var("DREAD_LLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11501".into()).trim_end_matches('/').to_string();
    let comfy_url = env::var("DREAD_COMFYUI_URL").unwrap_or_else(|_| "http://127.0.0.1:8188".into()).trim_end_matches('/').to_string();
    let model = env::var("DREAD_MODEL").unwrap_or_else(|_| "qwen3.8:27b".into());
    let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).build()?;
    let state = Arc::new(AppState { http, llama_url, comfy_url, model });
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/services", get(services))
        .route("/api/models", get(models))
        .route("/api/chat", post(chat))
        .route("/api/comfyui/status", get(comfy_status))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!("PROJECT DREAD UNIVERSAL API listening on http://{bind}; inference=llama.cpp");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_loopback_and_llama_cpp() {
        assert_eq!(env::var("DREAD_BIND").unwrap_or_else(|_| "127.0.0.1:8765".into()), "127.0.0.1:8765");
        assert_eq!(env::var("DREAD_LLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11501".into()), "http://127.0.0.1:11501");
    }
}
