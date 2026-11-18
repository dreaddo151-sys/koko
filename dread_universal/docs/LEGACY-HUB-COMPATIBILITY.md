# Legacy Master Hub Compatibility and Migration Gate

**Status:** Audit baseline only — not a completed parity implementation.  
**Reference source:** `legacy-reference/dread_web_hub.py`  
**Reference SHA-256:** `93ce30cacd58d90e03efd8ec6275c388cb8f9752` (Git blob SHA, not a SHA-256 digest).  
**Target platform:** Ubuntu 24.04 LTS and later.

## Scope and evidence boundary

The source reviewed is the repository's `legacy-reference/dread_web_hub.py`. It is a 5,915-line reference snapshot, not proof of the exact file currently installed at `/mnt/sabrent/dread_workspace/dread_web_hub.py`. Before migration, capture the installed file's SHA-256 and compare it with the reference file. Do not overwrite or restart the live hub based on this snapshot alone.

The reference contains these route families:

| Legacy route family | Examples | Current Rust API relationship |
|---|---|---|
| Core chat and tools | `/api/chat`, `/api/run-tool`, `/api/generate` | No exact route parity. `/v1/chat/completions` is a different contract. |
| Upload and media | `/api/upload`, `/api/video-status`, `/api/stop-video` | No equivalent image/video job and artifact lifecycle in the Rust API. |
| Host/model/telemetry | `/api/telemetry`, `/api/models`, `/api/fetch`, `/api/hub/*`, `/v1/models`, `/api/tags`, `/api/model` | Partial concepts exist under `/api/system/profile`, `/api/gpu/*`, `/api/services`, `/api/registry`, and `/v1/models`; wire contracts differ. |
| Agent sessions and approvals | `/api/agent/*`, `/api/approval/*`, `/api/permission/*`, `/api/session/*` | `/api/agents` is discovery metadata, not a compatible session/permission API. |
| Hermes and DeerFlow | `/api/hermes/*`, `/api/deerflow/*` | Hermes has a library adapter contract, not a registered task runner. DeerFlow has a legacy Python worker only in this reference snapshot; it is not a verified Rust adapter. |
| Mission/task engine | `/api/build/*`, `/api/task*`, `/api/event`, `/api/threads` | Rust `/api/missions` and `/api/tasks` provide overlapping concepts, not complete route or payload compatibility. |
| Workspace and visual tools | `/api/workspace/*`, `/api/visual/*` | No full Rust route parity. |
| OpenAI-compatible model API | `/v1/models`, `/v1/models/status` | Rust implements `/v1/models` and `/v1/chat/completions`; `/v1/models/status` is not a matching route. |

The current Rust router includes system profile, GPU health/models and proxy, missions, tasks, capabilities/import, registry, agents, services, model/lease endpoints and health. Exact paths are in `crates/dread-api/src/main.rs`; this table deliberately distinguishes conceptual overlap from wire compatibility.

## Reference-source security observations requiring live confirmation

1. The reference server binds `ReusableTCPServer(("", PORT), ...)`, which listens on all interfaces unless constrained externally. Some API groups check loopback client addresses, but the source does not show a single authentication boundary protecting every route.
2. Core routes such as `/api/chat`, `/api/run-tool`, and `/api/upload` must be reviewed for exposure and authorization before any LAN-accessible deployment.
3. The `/outputs/` GET handler derives a filesystem path from the URL path and opens it when it exists. Test traversal and symlink escape against the exact installed source before relying on it.
4. The legacy DeerFlow worker uses a hard-coded `127.0.0.1:8001` backend and a fixed workspace `.env` path. Current upstream DeerFlow documents a different public API surface and deployment configuration. Treat the reference worker as unverified until its local deployment, auth mode, sandbox mode and API version are confirmed.
5. The legacy Hermes UI explicitly describes complete responses only; streaming progress and cancellation are not implemented in that UI path.

These are observations about the repository reference snapshot, not confirmed findings about the currently running workstation process.

## Gate 1 acceptance sequence

1. On the workstation, capture the live source hash without modifying it:
   `sha256sum /mnt/sabrent/dread_workspace/dread_web_hub.py`.
2. Compare the exact live source with this repository reference and record the deployed version, route inventory, service unit, bind address and process owner.
3. Build a route-by-route contract matrix with HTTP method, request/response schema, authentication, state/persistence, streaming, cancellation, artifact behavior and failure semantics.
4. For every route, mark one of: **native equivalent**, **safe proxy to preserved Python Hub**, **intentionally retired with replacement**, or **not migrated**. A similar route name is not proof of compatibility.
5. Test the candidate on an alternate loopback port with a backup and rollback plan. Do not stop or replace the Python Hub, ComfyUI, GPU Gateway, Supervisor Bridge or llama.cpp runtime during this phase.
6. Only after side-by-side acceptance may the default UI/entry point be switched. Preserve the Python Hub as rollback target until a reboot and recovery drill passes.

**Gate 1 remains open.** The Rust API is not yet a complete drop-in replacement for the reference Python Hub, and no live workstation source comparison or handoff test has been performed.
