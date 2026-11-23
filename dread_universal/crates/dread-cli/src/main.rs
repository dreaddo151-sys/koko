//! pstack — the PROJECT DREAD stack CLI.
//!
//! A thin, read-mostly operator client for the universal hub: see what the
//! hub sees (models, agents, skills, GPU, D-Spine lab, repair plan) from a
//! terminal. It only makes HTTP calls to the hub's own local API; it never
//! spawns processes and never approves or executes repairs. The only
//! mutating command is `sync skills`, which re-reads local skill roots the
//! same way the dashboard button does.
//!
//! Auth: `--token`, or DREAD_API_TOKEN (same token the hub was started
//! with). Hub base: `--url`, or DREAD_HUB_URL, or http://127.0.0.1:8081.

use anyhow::{Context, Result, bail};
use serde_json::Value;

const DEFAULT_URL: &str = "http://127.0.0.1:8081";

#[derive(Debug, PartialEq, Eq)]
struct Config {
    base_url: String,
    token: String,
    command: &'static str,
    method: &'static str,
    path: &'static str,
}

/// Pure argument parsing; no environment access (tested directly below).
fn parse_args(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Result<Config> {
    const COMMANDS: &[(&str, &str, &str)] = &[
        ("status", "GET", "/api/system/profile"),
        ("gpu", "GET", "/api/gpu/telemetry"),
        ("models", "GET", "/api/models/local"),
        ("agents", "GET", "/api/agents"),
        ("skills", "GET", "/api/skills"),
        ("dspine", "GET", "/api/dspine/status"),
        ("media", "GET", "/api/media/comfyui/monitor"),
        ("tokens", "GET", "/api/usage/tokens"),
        ("repair-plan", "GET", "/api/repair/plan"),
        ("sync-skills", "POST", "/api/skills/sync"),
    ];
    let mut base_url = env("DREAD_HUB_URL");
    let mut token = env("DREAD_API_TOKEN");
    let mut command: Option<&(&str, &str, &str)> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => {
                i += 1;
                base_url = Some(args.get(i).context("--url requires a value").cloned()?);
            }
            "--token" => {
                i += 1;
                token = Some(args.get(i).context("--token requires a value").cloned()?);
            }
            "-h" | "--help" | "help" => return Ok(Config { base_url: String::new(), token: String::new(), command: "help", method: "GET", path: "help" }),
            other => {
                let found = COMMANDS.iter().find(|(name, _, _)| *name == other);
                let Some(found) = found else {
                    bail!("unknown command {other:?} (try: {})", COMMANDS.iter().map(|c| c.0).collect::<Vec<_>>().join(", "));
                };
                if command.is_some() {
                    bail!("only one command per invocation");
                }
                command = Some(found);
            }
        }
        i += 1;
    }
    let Some(&(name, method, path)) = command else {
        return Ok(Config { base_url: String::new(), token: String::new(), command: "help", method: "GET", path: "help" });
    };
    Ok(Config {
        base_url: base_url.unwrap_or_else(|| DEFAULT_URL.to_string()),
        token: token.unwrap_or_default(),
        command: name,
        method,
        path,
    })
}

const HELP: &str = "\
pstack — PROJECT DREAD stack CLI (read-mostly; never executes repairs)

usage: pstack [--url URL] [--token TOKEN] <command>

commands:
  status        hub system profile (uptime, versions, feature flags)
  gpu           live GPU telemetry from the supervisor
  models        discovered local model inventory
  agents        configured + downloaded agent endpoints
  skills        skills visible to the hub
  dspine        D-Spine lab engine roots and draft trees
  media         ComfyUI monitor (queue, history, progress)
  tokens        token booster usage totals (provider-reported only)
  repair-plan   current findings and proposed actions (nothing is executed)
  sync-skills   rescan local skill roots (same as the dashboard button)

env: DREAD_HUB_URL (default http://127.0.0.1:8081), DREAD_API_TOKEN";

fn summarize(command: &str, value: &Value) -> String {
    // Compact human view first; full JSON follows from --raw? v1: summary
    // then pretty JSON is too noisy, so pick concise per-command digests.
    let count_of = |key: &str| value.get(key).and_then(Value::as_u64);
    match command {
        "gpu" => {
            let gpus = value.get("gpus").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            format!("{gpus} gpu(s) reported")
        }
        "models" => {
            let models = value.get("models").and_then(Value::as_array).map(Vec::len);
            match count_of("gguf_models_registered").or(models.map(|n| n as u64)) {
                Some(n) => format!("{n} local model entr(ies)"),
                None => "inventory reported".to_string(),
            }
        }
        "agents" => {
            let online = value.as_array().map(|a| a.iter().filter(|x| x.get("enabled").and_then(Value::as_bool) == Some(true)).count());
            match (value.as_array().map(Vec::len), online) {
                (Some(total), Some(on)) => format!("{total} agent endpoint(s), {on} enabled"),
                _ => "agents reported".to_string(),
            }
        }
        "skills" => {
            let n = value.get("skills").and_then(Value::as_array).map(Vec::len).or_else(|| value.as_array().map(Vec::len));
            match n {
                Some(n) => format!("{n} skill(s)"),
                None => "skills reported".to_string(),
            }
        }
        "dspine" => {
            let roots = value.get("engine_roots").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            let drafts = value.get("drafts").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            format!("{roots} engine root(s), {drafts} draft tree(s)")
        }
        "media" => match value.get("status").and_then(Value::as_str) {
            Some(s) => format!("comfyui monitor: {s}"),
            None => "media monitor reported".to_string(),
        },
        "tokens" => {
            let total = value.get("total_tokens").and_then(Value::as_u64).unwrap_or(0);
            let n = value.get("measured_responses").and_then(Value::as_u64).unwrap_or(0);
            format!("{total} token(s) across {n} measured response(s) (provider-reported only)")
        }
        "repair-plan" => {
            let findings = value.get("findings").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            format!("{findings} finding(s) — proposals only; approval and execution stay manual")
        }
        _ => String::new(),
    }
}

#[tokio::main]
async fn main() {
    // `pstack agents | head -1` is normal usage: die silently on EPIPE
    // instead of panicking in println!.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    match run(&args, &env).await {
        Ok(()) => {}
        Err(error) => {
            eprintln!("pstack: {error:#}");
            std::process::exit(1);
        }
    }
}

async fn run(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Result<()> {
    let config = parse_args(args, env)?;
    if config.path == "help" {
        println!("{HELP}");
        return Ok(());
    }
    let url = format!("{}{}", config.base_url.trim_end_matches('/'), config.path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("could not build HTTP client")?;
    let mut request = match config.method {
        "POST" => client.post(&url),
        _ => client.get(&url),
    };
    if !config.token.is_empty() {
        request = request.bearer_auth(&config.token);
    }
    let response = request.send().await.with_context(|| format!("{} {url} failed", config.method))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .with_context(|| format!("{url} returned non-JSON (HTTP {status})"))?;
    if !status.is_success() {
        bail!("hub returned HTTP {status}: {body}");
    }
    let summary = summarize(config.command, &body);
    if !summary.is_empty() {
        println!("{summary}");
    }
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_and_commands() {
        let c = parse_args(&args(&["agents"]), &no_env).unwrap();
        assert_eq!(c.base_url, DEFAULT_URL);
        assert_eq!((c.method, c.path), ("GET", "/api/agents"));
        let c = parse_args(&args(&["sync-skills"]), &no_env).unwrap();
        assert_eq!((c.method, c.path), ("POST", "/api/skills/sync"));
    }

    #[test]
    fn no_command_is_help() {
        let c = parse_args(&[], &no_env).unwrap();
        assert_eq!(c.path, "help");
    }

    #[test]
    fn overrides_win_env() {
        let env = |k: &str| match k {
            "DREAD_HUB_URL" => Some("http://env:1".to_string()),
            "DREAD_API_TOKEN" => Some("envtok".to_string()),
            _ => None,
        };
        let c = parse_args(&args(&["--url", "http://flag:2", "--token", "flagtok", "gpu"]), &env).unwrap();
        assert_eq!((c.base_url.as_str(), c.token.as_str()), ("http://flag:2", "flagtok"));
        assert_eq!(c.path, "/api/gpu/telemetry");
    }

    #[test]
    fn unknown_and_double_command_rejected() {
        assert!(parse_args(&args(&["rm-rf"]), &no_env).is_err());
        assert!(parse_args(&args(&["agents", "skills"]), &no_env).is_err());
        assert!(parse_args(&args(&["--url"]), &no_env).is_err());
    }
}
