# PROJECT DREAD Universal — portable runtime architecture

## Product target

A single local-first control plane that can be installed on a supported desktop OS, detects the host and available local runtimes, discovers local models and agent endpoints, and lets an operator add reviewed skills or agent profiles from a one-click extension catalog.

The Rust workspace is the portable control plane. GPU ownership remains behind an explicit runtime adapter; the API never silently starts a second inference server or takes ownership away from an existing supervisor.

## Current implementation in this branch

- Rust API, registry, task approval/audit, discovery and GPU gateway client.
- Read-only host discovery reports OS, architecture, logical CPU threads, known NVIDIA/AMD/Apple GPU metadata and a llama.cpp server executable found in PATH or configured by environment.
- `DREAD_GPU_GATEWAY_URL` configures the loopback OpenAI-compatible llama.cpp gateway. Default: `http://127.0.0.1:11500`.
- Active service discovery uses llama.cpp `/api/tags`; Ollama is not an active backend.
- Pstack scanner discovers `skills/<name>/SKILL.md` and optional `agents/<name>.md`.
- Imports are persisted as disabled capability records. Operators can preview the exact local file, approve it, disable it, and attach enabled content to an approved GPU chat task.
- Imported Markdown is untrusted context only. It cannot run scripts, shell commands, hooks, MCP tools or remote agents.
- Skill/profile content is size-limited and rechecked against a local change fingerprint before approval and use.
- Rust CI targets Linux, Windows and macOS. CI compile targets are not a claim that every platform-specific GPU runtime is already implemented.

## Runtime boundaries

### Model providers

The canonical inference backend is llama.cpp using an OpenAI-compatible local endpoint. Discovery queries `/v1/models` through the configured loopback gateway. A model is usable only when advertised by that gateway. No Ollama daemon or Ollama API is started.

### GPU ownership

The API talks to a configured loopback gateway and, for task inference, the authenticated Supervisor bridge. A platform installer must select exactly one GPU owner. It must never launch a second llama-server if a managed service is already active.

The current authenticated Python GPU Supervisor bridge is the workstation implementation. Windows and macOS require their own tested Supervisor/runtime adapters before they can be called fully supported for GPU inference. CPU-only llama.cpp remains a possible portable backend once its lifecycle adapter is implemented.

### Agents versus prompt profiles

A live agent is a process or service with a reviewed adapter implementing the AgentAdapter lifecycle (health, start, status, events, cancellation and result). A Markdown file under Pstack `agents/` is an agent prompt profile, not a running agent. The current extension manager imports such files as untrusted context profiles; it does not pretend to launch a remote agent.

Live agent discovery must use explicit known local endpoints or operator-configured endpoints. Do not sweep arbitrary ports, contact arbitrary LAN hosts, or launch discovered executables.

### Skills and extensions

The current Pstack integration uses a local catalog root configured by `DREAD_SKILLS_ROOT`. Import is metadata-only and disabled by default. Enabling requires operator identity, a review reason, and an unchanged source fingerprint. Enabled content is injected only into an approved GPU chat task under a trusted DREAD policy message.

Future remote extension installation must use a signed/pinned manifest, explicit source allowlist, staged download, checksum/signature verification, backup, atomic activation and rollback. It must not execute install hooks from imported plugins.

## Cross-platform packaging roadmap

| Phase | Deliverable | Acceptance evidence |
|---|---|---|
| 1 | Portable Rust core and CI | fmt, Clippy, tests and release build pass on Linux, Windows and macOS |
| 2 | Host profile discovery | OS, architecture, CPU threads, supported GPU vendor/model/VRAM and available llama.cpp binary are detected without changing the host; RAM and additional vendor adapters remain to be added |
| 3 | Runtime adapters | llama.cpp lifecycle, health, model discovery and shutdown are tested per OS; one GPU owner invariant is enforced |
| 4 | Installers | Linux shell, Windows PowerShell and macOS package installers create private config, service startup and rollback |
| 5 | Agent catalog | Known local agent endpoints are probed read-only; adding an agent requires an adapter manifest and explicit enablement |
| 6 | Extension manager | One-click discover, preview, install, update, disable and rollback for pinned skills and agent profiles |
| 7 | Release artifacts | Signed versioned executables/installers and a verified update manifest for each supported OS/architecture |

## Portability contract

- Never assume a fixed home directory, workspace path, GPU vendor, GPU count, CUDA version or service manager.
- Resolve paths from environment/config and the executable's installation root.
- Keep secrets outside the repository with restrictive file permissions or the OS credential store.
- Use loopback-only defaults; remote access requires a separate authentication and network threat model.
- Detect optional capabilities and report them as unavailable rather than fabricating health or silently falling back to a different model provider.
- Installer/update operations are additive, backed up, and reversible.
