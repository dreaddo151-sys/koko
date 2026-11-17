#!/usr/bin/env bash
# Read-only PROJECT DREAD workstation acceptance probe.
# Does not stop, restart, kill, reconfigure or modify any service/GPU.
set -u
LLAMA_URL="${DREAD_LLAMA_URL:-http://127.0.0.1:11501}"
COMFY_URL="${DREAD_COMFYUI_URL:-http://127.0.0.1:8188}"
API_URL="${DREAD_API_URL:-http://127.0.0.1:8765}"
SUPERVISOR_URL="${DREAD_GPU_SUPERVISOR_URL:-http://127.0.0.1:11500}"
HUB_URL="${DREAD_LEGACY_HUB_URL:-http://127.0.0.1:8080}"
fail=0
probe() {
  local label="$1" url="$2" required="$3" code
  code="$(curl --connect-timeout 3 --max-time 8 -sS -o /dev/null -w '%{http_code}' "$url" 2>/dev/null || true)"
  if [[ "$code" =~ ^2|3 ]]; then printf '[ONLINE]  %-22s HTTP %s\n' "$label" "$code"
  else printf '[OFFLINE] %-22s %s\n' "$label" "${code:-no response}"; [[ "$required" == yes ]] && fail=1; fi
}
echo "=== PROJECT DREAD READ-ONLY ACCEPTANCE ==="
echo "No services will be stopped or changed."
echo
probe "llama.cpp health" "$LLAMA_URL/health" yes
probe "llama.cpp models" "$LLAMA_URL/v1/models" yes
probe "llama.cpp tokenizer" "$LLAMA_URL/tokenize" no
probe "ComfyUI" "$COMFY_URL/system_stats" yes
probe "Rust API" "$API_URL/health" no
probe "GPU Supervisor" "$SUPERVISOR_URL/health" no
probe "Legacy Hub" "$HUB_URL/" no
echo
echo "=== TOKENIZER POST TEST ==="
token_response="$(curl --connect-timeout 3 --max-time 15 -sS -X POST "$LLAMA_URL/tokenize" -H 'Content-Type: application/json' --data '{"content":"PROJECT DREAD tokenizer acceptance test"}' 2>&1 || true)"
if printf '%s' "$token_response" | python3 -c 'import json,sys; d=json.load(sys.stdin); t=d.get("tokens") if isinstance(d,dict) else d; assert isinstance(t,list); print("Exact token count:",len(t))' 2>/dev/null; then :; else echo "Tokenizer POST failed or returned an unsupported response."; fail=1; fi
echo
echo "=== NVIDIA GPU INVENTORY ==="
if command -v nvidia-smi >/dev/null 2>&1; then nvidia-smi --query-gpu=index,name,uuid,memory.total,memory.free --format=csv
else echo "nvidia-smi not found"; fail=1; fi
echo
echo "=== PCIe LINK STATUS (READ ONLY) ==="
if command -v lspci >/dev/null 2>&1; then
  lspci -Dvv 2>/dev/null | awk '/NVIDIA/{show=1;print;next} show && /LnkCap:|LnkSta:/{print} show && /^$/{show=0}'
else echo "lspci not found (optional)"; fi
echo
if [[ "$fail" -eq 0 ]]; then echo "REQUIRED ACCEPTANCE PROBES PASSED"; else echo "ONE OR MORE REQUIRED PROBES FAILED; no services were modified."; fi
exit "$fail"
