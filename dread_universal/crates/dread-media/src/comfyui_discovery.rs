//! ComfyUI auto-discovery and metadata snapshot for the local-first AI hub.
//!
//! This module is deliberately conservative: an endpoint is only reported as
//! ComfyUI when both `/system_stats` and `/queue` match the expected shapes.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;
use tokio::time::timeout;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CONCURRENT_PROBES: usize = 8;
/// Hard wall-clock bound for one full sweep (64 worst-case hostile candidates
/// at 8 slots x 10s budget = 80s; we stop at 60s and report what we have).
const SWEEP_DEADLINE: Duration = Duration::from_secs(60);
// One probe = up to three sequential requests; the sweep bounds the whole
// probe so a slow-but-valid endpoint is not killed by a single-request cap.
const PROBE_BUDGET: Duration = Duration::from_secs(10);
// Lead review fix: never let an endpoint stream an unbounded body into the
// hub process (OOM DoS on any LAN candidate). Cap + parse manually instead.
const SMALL_BODY_CAP: usize = 1 << 20;
const OBJECT_INFO_BODY_CAP: usize = 24 << 20;

async fn read_capped_json(
    resp: reqwest::Response,
    cap: usize,
) -> anyhow::Result<serde_json::Value> {
    // reqwest's .timeout() stops counting once response headers arrive, so a
    // hostile server that trickles the body can hang the read forever. Bound
    // the body read itself.
    let acc = timeout(PROBE_TIMEOUT, read_capped_bytes(resp, cap))
        .await
        .map_err(|_| anyhow::anyhow!("response body read timed out"))??;
    Ok(serde_json::from_slice(&acc)?)
}

async fn read_capped_bytes(mut resp: reqwest::Response, cap: usize) -> anyhow::Result<Vec<u8>> {
    let mut acc: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if acc.len() + chunk.len() > cap {
            anyhow::bail!("response body exceeds {cap} bytes");
        }
        acc.extend_from_slice(&chunk);
    }
    Ok(acc)
}
const HEALTHY_REFRESH: Duration = Duration::from_secs(30);
const DEGRADED_REFRESH: Duration = Duration::from_secs(120);
const MAX_BACKOFF_REFRESH: Duration = Duration::from_secs(300);

/// A single accelerator/device entry from ComfyUI's `/system_stats`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceVram {
    pub name: String,
    pub index: i64,
    pub vram_total: u64,
    pub vram_free: u64,
}

/// Facts gathered from a verified ComfyUI endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ComfyUiFacts {
    pub base_url: String,
    pub identity: String,
    pub comfyui_version: String,
    pub os: String,
    pub devices: Vec<DeviceVram>,
    pub node_type_count: Option<usize>,
    pub checkpoints: Vec<String>,
    pub probe_latency_ms: u64,
}

/// Refresh cadence policy for a discovered endpoint.
///
/// The hub owns the consecutive-failure counter and the manual re-enable flag;
/// this type only documents/encapsulates the policy values.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct RefreshPolicy {
    pub healthy: Duration,
    pub degraded: Duration,
    pub max_backoff: Duration,
    pub degraded_after_failures: u32,
    pub offline_after_failures: u32,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            healthy: HEALTHY_REFRESH,
            degraded: DEGRADED_REFRESH,
            max_backoff: MAX_BACKOFF_REFRESH,
            degraded_after_failures: 3,
            offline_after_failures: 10,
        }
    }
}

/// Pure refresh calculation.
///
/// 0 failures => healthy (30s), 1-2 => degraded (120s), 3-9 => capped backoff
/// (300s max), and >=10 => a very large duration representing "offline until
/// manual re-enable". Callers should stop scheduling probes entirely at >=10.
pub fn next_refresh(consecutive_failures: u32) -> Duration {
    match consecutive_failures {
        0 => HEALTHY_REFRESH,
        1 | 2 => DEGRADED_REFRESH,
        3..=9 => {
            // Exponential backoff from the degraded interval, capped at 300s.
            // 3 -> 240s, 4+ -> 300s (cap).
            let shift = consecutive_failures.saturating_sub(3);
            let multiplier = 2u64.checked_shl(shift).unwrap_or(u64::MAX);
            let millis = DEGRADED_REFRESH
                .as_millis()
                .saturating_mul(u128::from(multiplier));
            let capped = millis.min(MAX_BACKOFF_REFRESH.as_millis());
            Duration::from_millis(capped as u64)
        }
        _ => Duration::from_secs(365 * 24 * 60 * 60),
    }
}

/// Normalize a base URL for identity purposes: lowercase scheme, no trailing
/// slash. Host casing is left to `reqwest`/URL parsing during probes; for the
/// pure identity function we lowercase the scheme and authority defensively.
pub fn identity_for(base_url: &str) -> Option<String> {
    let normalized = normalize_base_url(base_url)?;
    let digest = ring::digest::digest(&ring::digest::SHA256, normalized.as_bytes());
    let hex = digest
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Some(format!("comfy-{hex}"))
}

/// Extract `(node_type_count, checkpoints)` from a 2.7MB `/object_info` value
/// without retaining anything beyond the returned summary.
pub fn snapshot_checkpoints(object_info: &Value) -> (usize, Vec<String>) {
    let Some(map) = object_info.as_object() else {
        return (0, Vec::new());
    };
    let node_type_count = map.len();

    let mut basenames = BTreeSet::new();
    if let Some(entries) = map
        .get("CheckpointLoaderSimple")
        .and_then(|node| node.get("input"))
        .and_then(|input| input.get("required"))
        .and_then(|required| required.get("ckpt_name"))
        .and_then(|ckpt| ckpt.get(0))
        .and_then(Value::as_array)
    {
        for entry in entries {
            if let Some(raw) = entry.as_str() {
                let base = basename(raw);
                if !base.is_empty() {
                    basenames.insert(base);
                }
            }
        }
    }

    (node_type_count, basenames.into_iter().collect())
}

/// Probe one candidate. Returns `None` unless both `/system_stats` and `/queue`
/// validate. `/object_info` is best-effort and never affects detection.
pub async fn probe_comfyui(client: &reqwest::Client, base_url: &str) -> Option<ComfyUiFacts> {
    let started = Instant::now();
    let base = normalize_base_url(base_url)?;

    let stats_url = format!("{base}/system_stats");
    let queue_url = format!("{base}/queue");

    let stats_resp = client
        .get(&stats_url)
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
        .ok()?;
    if stats_resp.status() != StatusCode::OK {
        return None;
    }
    let stats: Value = read_capped_json(stats_resp, SMALL_BODY_CAP).await.ok()?;

    let queue_resp = client
        .get(&queue_url)
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
        .ok()?;
    if queue_resp.status() != StatusCode::OK {
        return None;
    }
    let queue: Value = read_capped_json(queue_resp, SMALL_BODY_CAP).await.ok()?;

    // Both keys must exist and be arrays to confirm ComfyUI.
    let queue_ok = queue.get("queue_running").is_some_and(Value::is_array)
        && queue.get("queue_pending").is_some_and(Value::is_array);
    if !queue_ok {
        return None;
    }

    let system = stats.get("system")?;
    let comfyui_version = system.get("comfyui_version")?.as_str()?.to_owned();
    if comfyui_version.is_empty() {
        return None;
    }
    let os = system
        .get("os")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    let devices = stats
        .get("devices")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some(DeviceVram {
                        name: d.get("name")?.as_str()?.to_owned(),
                        index: d.get("index")?.as_i64()?,
                        vram_total: d.get("vram_total")?.as_u64()?,
                        vram_free: d.get("vram_free")?.as_u64()?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Best-effort metadata snapshot; never fails detection.
    let object_info_url = format!("{base}/object_info");
    let (node_type_count, checkpoints) = match client
        .get(&object_info_url)
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
    {
        Ok(resp) if resp.status() == StatusCode::OK => {
            match read_capped_json(resp, OBJECT_INFO_BODY_CAP).await {
                Ok(info) => {
                    let (count, ckpts) = snapshot_checkpoints(&info);
                    (Some(count), ckpts)
                }
                Err(_) => (None, Vec::new()),
            }
        }
        _ => (None, Vec::new()),
    };

    let identity = identity_for(&base)?;
    Some(ComfyUiFacts {
        base_url: base,
        identity,
        comfyui_version,
        os,
        devices,
        node_type_count,
        checkpoints,
        probe_latency_ms: started.elapsed().as_millis() as u64,
    })
}

/// Probe candidates with bounded concurrency (<=8) and a hard per-probe 3s
/// timeout. Results are sorted by `base_url` for stable registry ordering.
pub async fn discover_endpoints(
    client: &reqwest::Client,
    candidates: &[String],
) -> Vec<ComfyUiFacts> {
    let client = Arc::new(client.clone());
    let mut set: JoinSet<Option<ComfyUiFacts>> = JoinSet::new();
    let mut iter = candidates.iter();
    let mut results = Vec::new();

    let sweep = async {
        loop {
            while set.len() < MAX_CONCURRENT_PROBES {
                let Some(candidate) = iter.next() else { break };
                let client = Arc::clone(&client);
                let candidate = candidate.clone();
                set.spawn(async move {
                    timeout(PROBE_BUDGET, probe_comfyui(&client, &candidate))
                        .await
                        .unwrap_or_default()
                });
            }

            if set.is_empty() {
                return;
            }
            if let Some(Ok(Some(facts))) = set.join_next().await {
                results.push(facts);
            }
            if set.is_empty() && iter.len() == 0 {
                return; // trait-len: ExactSizeIterator::is_empty is unstable
            }
        }
    };
    // On deadline we lose nothing: probes already collected are kept, and the
    // dropped JoinSet aborts the stragglers.
    let _ = timeout(SWEEP_DEADLINE, sweep).await;

    results.sort_by(|a, b| a.base_url.cmp(&b.base_url));
    results
}

/// Canonicalize a ComfyUI base URL: lowercased scheme+host, canonical bracketed
/// IPv6, default port stripped, path preserved without trailing slash.
/// Rejects (returns `None`): non-http(s) schemes, missing host, and embedded
/// credentials - a secret must never be hashed into an identity nor silently
/// mutated by case-folding.
pub fn normalize_base_url(input: &str) -> Option<String> {
    let raw = input.trim();
    let with_scheme = if raw.contains("://") {
        std::borrow::Cow::Borrowed(raw)
    } else {
        std::borrow::Cow::Owned(format!("http://{raw}"))
    };
    let url = url::Url::parse(with_scheme.as_ref()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    if host.is_empty() {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    // url::Url already lowercased the host, canonicalized IPv6 (keeping its
    // required brackets), and dropped default ports during parsing.
    let host_part = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let mut out = format!("{}://{host_part}", url.scheme());
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    out.push_str(url.path().trim_end_matches('/'));
    Some(out)
}

fn basename(path: &str) -> String {
    let trimmed = path.trim();
    let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    without_query
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(without_query)
        .trim()
        .to_owned()
}

/// Helper for the hub: fetch and snapshot `/object_info` on demand.
pub async fn fetch_object_info_snapshot(
    client: &reqwest::Client,
    base_url: &str,
) -> anyhow::Result<(usize, Vec<String>)> {
    let base = normalize_base_url(base_url)
        .ok_or_else(|| anyhow::anyhow!("invalid ComfyUI base URL: {base_url}"))?;
    let url = format!("{base}/object_info");
    let value: Value = read_capped_json(
        client
            .get(url)
            .timeout(PROBE_TIMEOUT)
            .send()
            .await
            .context("object_info request failed")?
            .error_for_status()
            .context("object_info returned error status")?,
        OBJECT_INFO_BODY_CAP,
    )
    .await
    .context("object_info body")?;
    Ok(snapshot_checkpoints(&value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const STATS_OK: &str = r#"{"system":{"os":"linux","comfyui_version":"0.43.0"},"devices":[{"name":"cuda:0 NVIDIA GeForce RTX 3090 : cudaMallocAsync","type":"cuda","index":0,"vram_total":25342705664,"vram_free":381222912,"torch_vram_total":0,"torch_vram_free":0}]}"#;
    const QUEUE_OK: &str = r#"{"queue_running":[],"queue_pending":[]}"#;
    const OBJECT_INFO: &str = r#"{"CheckpointLoaderSimple":{"input":{"required":{"ckpt_name":[["models/checkpoints/a.safetensors","b.safetensors","a.safetensors"],"other"]}}},"OtherNode":{}}"#;

    async fn serve(routes: &'static [(&'static str, &'static str, &'static str)]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .split('?')
                        .next()
                        .unwrap_or("/");
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _, _)| *p == path)
                        .map(|(_, s, b)| (*s, *b))
                        .unwrap_or(("404 Not Found", "{}"));
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn probe_accepts_full_shape() {
        let base = serve(&[
            ("/system_stats", "200 OK", STATS_OK),
            ("/queue", "200 OK", QUEUE_OK),
            ("/object_info", "200 OK", OBJECT_INFO),
        ])
        .await;
        let facts = probe_comfyui(&client(), &base).await.expect("must accept");
        assert_eq!(facts.comfyui_version, "0.43.0");
        assert_eq!(facts.os, "linux");
        assert_eq!(facts.devices.len(), 1);
        assert_eq!(facts.devices[0].vram_total, 25_342_705_664);
        assert_eq!(facts.node_type_count, Some(2));
        assert_eq!(
            facts.checkpoints,
            vec!["a.safetensors".to_owned(), "b.safetensors".to_owned()]
        );
        assert!(facts.identity.starts_with("comfy-"));
        assert_eq!(facts.identity.len(), "comfy-".len() + 64);
    }

    #[tokio::test]
    async fn probe_rejects_queue_missing_keys() {
        let base = serve(&[
            ("/system_stats", "200 OK", STATS_OK),
            ("/queue", "200 OK", r#"{"queue_running":[]}"#),
        ])
        .await;
        assert!(probe_comfyui(&client(), &base).await.is_none());
    }

    #[tokio::test]
    async fn probe_rejects_stats_without_version() {
        let base = serve(&[
            (
                "/system_stats",
                "200 OK",
                r#"{"system":{"os":"linux"},"devices":[]}"#,
            ),
            ("/queue", "200 OK", QUEUE_OK),
        ])
        .await;
        assert!(probe_comfyui(&client(), &base).await.is_none());
    }

    #[tokio::test]
    async fn discover_filters_dead_and_sorts() {
        let live_b = serve(&[
            ("/system_stats", "200 OK", STATS_OK),
            ("/queue", "200 OK", QUEUE_OK),
        ])
        .await;
        let live_a = serve(&[
            ("/system_stats", "200 OK", STATS_OK),
            ("/queue", "200 OK", QUEUE_OK),
        ])
        .await;
        // Dead port: bind then drop.
        let dead = {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let a = l.local_addr().unwrap();
            drop(l);
            format!("http://{a}")
        };
        let candidates = vec![live_b.clone(), dead, live_a.clone()];
        let mut facts = discover_endpoints(&client(), &candidates).await;
        facts.sort_by(|a, b| a.base_url.cmp(&b.base_url));
        assert_eq!(facts.len(), 2);
        let mut expected = [live_a, live_b];
        expected.sort();
        assert_eq!(facts[0].base_url, expected[0]);
        assert_eq!(facts[1].base_url, expected[1]);
    }

    #[test]
    fn identity_stable_and_normalized() {
        let a = identity_for("HTTP://LOCALHOST:8188/").unwrap();
        let b = identity_for("http://localhost:8188").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 6 + 64);
        assert!(a.chars().skip(6).all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, identity_for("http://localhost:8189").unwrap());
    }

    #[test]
    fn identity_table_from_hostile_review() {
        // Canonical spelling itself, not just hash consistency.
        assert_eq!(
            normalize_base_url("http://[0:0:0:0:0:0:0:1]:8188").as_deref(),
            Some("http://[::1]:8188")
        );
        assert_eq!(
            normalize_base_url("HTTP://LOCALHOST:8188/").as_deref(),
            Some("http://localhost:8188")
        );
        assert_eq!(
            normalize_base_url("http://host:80/x/").as_deref(),
            Some("http://host/x")
        );
        // Case, whitespace, trailing slash: same server.
        assert_eq!(
            identity_for("  HTTP://LOCALHOST:8188/  ").unwrap(),
            identity_for("http://localhost:8188").unwrap()
        );
        // Default ports are stripped, not hashed.
        assert_eq!(
            identity_for("http://host:80").unwrap(),
            identity_for("http://host").unwrap()
        );
        assert_eq!(
            identity_for("https://host:443").unwrap(),
            identity_for("https://host").unwrap()
        );
        // The same IPv6 server in two spellings is one identity.
        assert_eq!(
            identity_for("http://[0:0:0:0:0:0:0:1]:8188").unwrap(),
            identity_for("http://[::1]:8188").unwrap()
        );
        // Credentials are rejected outright: never hashed, never case-folded.
        assert!(identity_for("http://User:Secret@host:8188").is_none());
        // Non-http(s) schemes are rejected, not silently probed.
        assert!(identity_for("ftp://host:8188").is_none());
        assert!(identity_for("ws://host:8188").is_none());
        assert!(identity_for("file:///tmp/comfy").is_none());
    }

    #[tokio::test]
    async fn trickle_body_cannot_hang_a_probe() {
        // Hostile server: valid headers, then holds the body open forever.
        // reqwest's per-request timeout stops at headers, so without the
        // inner body-read timeout this probe would hang until PROBE_BUDGET.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = socket.read(&mut buf).await;
                    // Valid headers + an unterminated "{" that is never
                    // finished: a body read with no timeout hangs here.
                    let head = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{";
                    let _ = socket.write_all(head.as_bytes()).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                });
            }
        });
        let url = format!("http://{addr}");
        let started = Instant::now();
        let facts = probe_comfyui(&client(), &url).await;
        let elapsed = started.elapsed();
        assert!(facts.is_none(), "trickling server must not yield facts");
        assert!(
            elapsed < Duration::from_secs(8),
            "probe took {elapsed:?}; body-read timeout missing"
        );
        server.abort();
    }

    #[test]
    fn snapshot_dedups_and_tolerates_garbage() {
        let v: Value = serde_json::from_str(OBJECT_INFO).unwrap();
        let (count, ckpts) = snapshot_checkpoints(&v);
        assert_eq!(count, 2);
        assert_eq!(ckpts, vec!["a.safetensors", "b.safetensors"]);

        let (count, ckpts) = snapshot_checkpoints(&serde_json::json!([1, 2, 3]));
        assert_eq!(count, 0);
        assert!(ckpts.is_empty());

        let (count, ckpts) = snapshot_checkpoints(&serde_json::json!({
            "CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": "nope"}}},
            "X": 1
        }));
        assert_eq!(count, 2);
        assert!(ckpts.is_empty());
    }

    #[test]
    fn refresh_policy_thresholds() {
        assert_eq!(next_refresh(0), Duration::from_secs(30));
        assert_eq!(next_refresh(1), Duration::from_secs(120));
        assert_eq!(next_refresh(3), Duration::from_secs(240));
        assert_eq!(next_refresh(4), Duration::from_secs(300));
        assert_eq!(next_refresh(9), Duration::from_secs(300));
        assert!(next_refresh(10) > Duration::from_secs(300));
    }
}
