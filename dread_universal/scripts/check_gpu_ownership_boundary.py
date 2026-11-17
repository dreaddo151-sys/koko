#!/usr/bin/env python3
"""Regression guard for single-owner GPU lease and direct-upstream boundaries."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUST_ROOT = ROOT / "crates"
rust_files = sorted(RUST_ROOT.glob("**/*.rs"))
assert rust_files, "Rust source files were not found"

for path in rust_files:
    source = path.read_text(encoding="utf-8")
    assert "11501" not in source, f"{path}: Rust must not call the tensor upstream directly"
    assert "acquire_tensor_lease" not in source, f"{path}: Rust must not acquire a second GPU lease"
    assert "release_lease" not in source, f"{path}: Rust must not release the Supervisor-owned lease"
    assert "GpuLeaseGuard" not in source, f"{path}: Rust must not manage GPU lease guards"
    assert "flock" not in source, f"{path}: Rust must not create a second GPU lock"

gpu_client = (RUST_ROOT / "dread-gpu/src/lib.rs").read_text(encoding="utf-8")
assert 'const DEFAULT_GATEWAY: &str = "http://127.0.0.1:11500";' in gpu_client
supervisor_client = (RUST_ROOT / "dread-gpu/src/supervisor.rs").read_text(encoding="utf-8")
assert "pub async fn chat_completion" in supervisor_client
assert "/v1/leases" not in supervisor_client

bridge = (ROOT / "scripts/dread_gpu_supervisor_bridge.py").read_text(encoding="utf-8")
assert 'TENSOR_UPSTREAM_URL = "http://127.0.0.1:11501/v1/chat/completions"' in bridge
assert 'with self.gpu_lease("tensor"):' in bridge
assert "http://127.0.0.1:11500/" not in bridge, "The lease-owning bridge must never call the lease-owning gateway"

supervisor = (ROOT.parent / "legacy-reference/dread_gpu_supervisor.py").read_text(encoding="utf-8")
assert "fcntl.flock(lockfile.fileno(), fcntl.LOCK_EX)" in supervisor
assert '["systemctl", "--user", action, "dread-tensor.service"]' in supervisor

gateway = (ROOT.parent / "legacy-reference/dread_gpu_gateway.py").read_text(encoding="utf-8")
assert "from dread_gpu_supervisor import gpu_lease" in gateway
assert 'UPSTREAM = "http://127.0.0.1:11501"' in gateway
assert 'with gpu_lease("tensor"):' in gateway

print("GPU ownership boundary: PASS")
print("Rust -> authenticated bridge :11502 -> one Python GPU lease -> managed tensor upstream :11501")
print("The lease-owning bridge does not call the lease-owning gateway :11500.")
