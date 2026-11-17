#!/usr/bin/env python3
"""Guard the single-owner GPU lease boundary between Rust and Python."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUST_ROOT = ROOT / "crates"
rust_files = sorted(RUST_ROOT.glob("**/*.rs"))
assert rust_files, "Rust source files were not found"

for path in rust_files:
    source = path.read_text(encoding="utf-8")
    assert "11501" not in source, f"{path}: Rust must not call the tensor upstream directly"
    assert "gpu_lease" not in source, f"{path}: Rust must not acquire the Python GPU lease"
    assert "flock" not in source, f"{path}: Rust must not create a second GPU lock"

gpu_client = (RUST_ROOT / "dread-gpu/src/lib.rs").read_text(encoding="utf-8")
assert 'const DEFAULT_GATEWAY: &str = "http://127.0.0.1:11500";' in gpu_client

# The preserved Python gateway is the sole lease boundary in the source baseline.
gateway = (ROOT.parent / "legacy-reference/dread_gpu_gateway.py").read_text(encoding="utf-8")
supervisor = (ROOT.parent / "legacy-reference/dread_gpu_supervisor.py").read_text(encoding="utf-8")
assert "from dread_gpu_supervisor import gpu_lease" in gateway
assert 'UPSTREAM = "http://127.0.0.1:11501"' in gateway
assert 'with gpu_lease("tensor"):' in gateway
assert "fcntl.flock(lockfile.fileno(), fcntl.LOCK_EX)" in supervisor
assert '["systemctl", "--user", action, "dread-tensor.service"]' in supervisor

print("GPU ownership boundary: PASS")
print("Rust -> loopback gateway :11500 -> Python GPU lease -> managed tensor upstream :11501")
print("No Rust-side lease, lock, or direct upstream access detected.")
