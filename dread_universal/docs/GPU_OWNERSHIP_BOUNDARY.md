# GPU ownership and inference boundary

## Required ownership model

There is exactly one GPU lease owner: the existing Python GPU Supervisor. Rust must not acquire or release leases, create a GPU lock, start or stop `dread-tensor.service`, or contact the tensor upstream directly.

The authenticated request path is:

`Rust API` -> `127.0.0.1:11502` (authenticated Supervisor bridge) -> `gpu_lease("tensor")` -> `127.0.0.1:11501` (managed tensor server).

The bridge acquires the Python lease and calls the managed tensor server directly. The legacy OpenAI-compatible gateway on `127.0.0.1:11500` also acquires the same lease for its own clients; it is used by Rust for model discovery, not as the bridge's inference upstream.

## Deadlock diagnosis and correction

The original bridge implementation acquired `gpu_lease("tensor")` and then posted inference to port 11500. That gateway also acquires `gpu_lease("tensor")`, so the same process path attempted to acquire the same exclusive file lock twice and could deadlock.

The bridge now posts directly to `127.0.0.1:11501/v1/chat/completions` while holding its single lease. Rust calls only the authenticated bridge for inference. The Rust client no longer exposes lease-acquire, lease-status, lease-release or RAII lease-guard methods. The gateway at 11500 remains available for model discovery and its own independent callers.

## Validation and limits

- Python bridge tests assert the exact upstream URL and prove the lease wraps the entire direct upstream request/response.
- Rust bridge-client tests verify bearer authentication and chat submission; Rust has no lease-management endpoints.
- `scripts/check_gpu_ownership_boundary.py` checks the source contract and runs in canonical-source CI.

These are source and mock-integration checks, not proof that the live workstation bridge is running or that a live GPU inference has succeeded. Workstation acceptance must verify the running gateway, Supervisor, bridge and managed service without stopping or reconfiguring ComfyUI, Ollama, the Python Hub or the GPU Supervisor.
