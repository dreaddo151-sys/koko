# GPU ownership and inference boundary

## Required ownership model

There is exactly one GPU lease owner: the existing Python GPU Supervisor. Rust must never acquire the file lock, call `gpu_lease`, start or stop `dread-tensor.service`, or contact the tensor upstream directly.

The intended request path is:

`Rust API / dread-gpu` -> `127.0.0.1:11500` (existing OpenAI-compatible Python gateway) -> `gpu_lease("tensor")` (Python Supervisor) -> `127.0.0.1:11501` (managed tensor server).

The gateway holds the lease across the full upstream request/response, including SSE streaming. The Supervisor performs ComfyUI-idle checks, ComfyUI memory release, managed systemd handoff, and cleanup. Rust is only a gateway client.

## Deadlock resolution

Do not add a Rust-called Python bridge that acquires `gpu_lease` and then calls port 11500. The gateway itself acquires the same exclusive lease, so that proposed nested path would block on its own lock. The safe correction is to keep the gateway as the only lease boundary and have Rust call it directly.

The current Rust GPU client defaults to loopback port 11500, rejects non-loopback gateway URLs, disables redirects, and exposes bounded health/model/inference requests. It does not import or acquire the Python lease. Port 11501 is upstream-only and must never be configured in Rust.

## Validation and limits

`scripts/check_gpu_ownership_boundary.py` statically checks the Rust/Python source contract and is run in canonical-source CI. It detects direct Rust references to port 11501 or a second lease/lock and verifies that the preserved Python gateway owns the lease.

This is source-level architecture validation, not proof that the live workstation services currently match these files. Workstation acceptance must separately verify the running gateway, Supervisor and managed service without stopping or reconfiguring them. No GPU service migration or restart is part of this change.
