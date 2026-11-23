#!/usr/bin/env bash
set -euo pipefail
umask 077

for command in systemctl curl ss df date uname hostname dpkg install; do
  command -v "$command" >/dev/null 2>&1 || {
    printf 'Missing baseline command: %s\n' "$command" >&2
    exit 1
  }
done

if [[ ! -r /etc/os-release ]]; then
  printf 'PROJECT DREAD supports Ubuntu 24.04 LTS and later only.\n' >&2
  exit 1
fi
# shellcheck disable=SC1091
. /etc/os-release
if [[ "${ID:-}" != ubuntu ]] || ! dpkg --compare-versions "${VERSION_ID:-0}" ge 24.04; then
  printf 'Unsupported operating system: %s (requires Ubuntu 24.04 or later).\n' "${PRETTY_NAME:-unknown}" >&2
  exit 1
fi

WORKSPACE="${DREAD_WORKSPACE:-$HOME/dread_workspace}"
if [[ -d /mnt/sabrent/dread_workspace && -z "${DREAD_WORKSPACE:-}" ]]; then
  WORKSPACE=/mnt/sabrent/dread_workspace
fi
BASE_DIR="$HOME/.local/share/dread-universal/baselines"
install -d -m 0700 "$BASE_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$BASE_DIR/workstation-baseline-$STAMP.txt"

{
  printf 'PROJECT DREAD PRESERVATION BASELINE\n'
  printf 'Captured UTC: %s\n' "$(date -u -Is)"
  printf 'Hostname: %s\n' "$(hostname)"
  printf 'Kernel: %s\n' "$(uname -a)"
  printf 'OS: %s\n' "${PRETTY_NAME:-unknown}"
  printf 'Architecture: %s\n' "$(dpkg --print-architecture)"
  printf 'Workspace: %s\n' "$WORKSPACE"
  printf '\n=== USER SERVICES (READ ONLY) ===\n'
  systemctl --user list-units --type=service --all --no-pager 2>&1 \
    | grep -Ei 'dread|comfy|llama|openhands|agent' || true
  printf '\n=== REQUIRED SERVICE STATES ===\n'
  for unit in dread-gpu-supervisor-bridge.service dread-universal-api.service dread-tensor.service; do
    printf '%s: ' "$unit"
    systemctl --user is-active "$unit" 2>&1 || true
  done
  printf '\n=== LISTENING TCP PORTS ===\n'
  ss -ltnp 2>&1 | grep -E ':(8080|8188|8765|11500|11501|11502)\b' || true
  printf '\n=== HTTP ENDPOINT STATUS (NO TOKENS/RESPONSE BODIES) ===\n'
  for entry in \
    'DREAD legacy Hub|http://127.0.0.1:8080/' \
    'DREAD telemetry|http://127.0.0.1:8080/api/telemetry' \
    'ComfyUI|http://127.0.0.1:8188/system_stats' \
    'GPU Supervisor gateway|http://127.0.0.1:11500/' \
    'Authenticated bridge|http://127.0.0.1:11502/health' \
    'Rust API|http://127.0.0.1:8765/health'; do
    label="${entry%%|*}"
    url="${entry#*|}"
    code="$(curl --connect-timeout 2 --max-time 3 --silent --output /dev/null --write-out '%{http_code}' "$url" 2>/dev/null || true)"
    printf '%s: HTTP %s\n' "$label" "${code:-unreachable}"
  done
  printf '\n=== CPU TOPOLOGY ===\n'
  if command -v lscpu >/dev/null 2>&1; then
    lscpu --json 2>&1 || lscpu 2>&1 || true
  else
    grep -E '^(processor|model name|physical id|core id):' /proc/cpuinfo 2>&1 || true
  fi
  printf '\n=== NVIDIA DEVICES / LINK STATE ===\n'
  printf 'PCIe current link values are a point-in-time snapshot; idle power-saving downshift is expected. Validate link generation/width only under sustained GPU load.\n'
  if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi -L 2>&1 || true
    nvidia-smi --query-gpu=name,memory.total,memory.used,driver_version,pcie.link.gen.current,pcie.link.width.current,pcie.link.gen.max,pcie.link.width.max,utilization.gpu,temperature.gpu,power.draw --format=csv,noheader,nounits 2>&1 || true
  else
    printf 'nvidia-smi: not found\n'
  fi
  printf '\n=== CUDA TOOLKIT DISCOVERY ===\n'
  for nvcc in "$(command -v nvcc 2>/dev/null || true)" /usr/local/cuda/bin/nvcc /usr/local/cuda-*/bin/nvcc; do
    if [[ -n "$nvcc" && -x "$nvcc" ]]; then
      printf 'Compiler: %s\n' "$nvcc"
      "$nvcc" --version 2>&1 || true
    fi
  done
  printf '\n=== PHYSICAL BLOCK DEVICES (INCLUDING UNMOUNTED DISKS) ===\n'
  if command -v lsblk >/dev/null 2>&1; then
    lsblk --json --bytes --output NAME,TYPE,SIZE,FSTYPE,MOUNTPOINTS,MODEL,TRAN 2>&1 || true
  else
    printf 'lsblk: not found\n'
  fi
  printf '\n=== WORKSPACE / DATA FILESYSTEM CAPACITY ===\n'
  if [[ -d "$WORKSPACE" ]]; then
    df -h "$WORKSPACE" 2>&1 || true
    for path in "$WORKSPACE" "$WORKSPACE/ComfyUI" "$HOME/dread_rust_redesign" "$HOME/dread_rust_stage"; do
      [[ -e "$path" ]] && ls -ld "$path"
    done
  else
    printf 'Workspace directory not found: %s\n' "$WORKSPACE"
  fi
  printf '\n=== END BASELINE ===\n'
} | tee "$OUT"

chmod 0600 "$OUT"
printf '\nBaseline saved: %s\n' "$OUT"
printf 'This script does not start, stop, or restart any service.\n'
