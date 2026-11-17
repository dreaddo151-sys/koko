
from contextlib import contextmanager
from pathlib import Path
import fcntl, json, os, socket, subprocess, time, urllib.request

ROOT = Path(__file__).resolve().parent
LOCK = ROOT / ".dread_gpu.lock"
TENSOR_HEALTH = "http://127.0.0.1:11501/health"
COMFY = "http://127.0.0.1:8188"

def _get(url, timeout=5):
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return json.loads(r.read().decode())

def _post(url, payload, timeout=90):
    data = json.dumps(payload).encode()
    req = urllib.request.Request(
        url, data=data, method="POST",
        headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()

def _queue_empty():
    q = _get(COMFY + "/queue")
    return not q.get("queue_running") and not q.get("queue_pending")

def _wait_comfy_idle(timeout=3600):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if _queue_empty():
            return
        time.sleep(2)
    raise TimeoutError("ComfyUI did not become idle; GPU handoff cancelled.")


def _service(action):
    subprocess.run(
        ["systemctl", "--user", action, "dread-tensor.service"],
        check=True, timeout=120)

def _wait_tensor(online, timeout=180):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            _get(TENSOR_HEALTH, timeout=2)
            state = True
        except Exception:
            state = False
        if state == online:
            return
        time.sleep(1)
    raise TimeoutError("Tensor server did not reach requested state.")

@contextmanager
def gpu_lease(mode):
    if mode not in ("tensor", "comfy"):
        raise ValueError("mode must be 'tensor' or 'comfy'")

    LOCK.parent.mkdir(parents=True, exist_ok=True)

    with LOCK.open("a+") as lockfile:
        fcntl.flock(lockfile.fileno(), fcntl.LOCK_EX)
        started_here = False

        try:
            if mode == "tensor":
                _wait_comfy_idle()
                _post(
                    COMFY + "/free",
                    {"unload_models": True, "free_memory": True},
                    timeout=120
                )

                # The systemd service is the only permitted server owner.
                active = subprocess.run(
                    ["systemctl", "--user", "is-active", "--quiet",
                     "dread-tensor.service"],
                    check=False
                ).returncode == 0

                if not active:
                    # Refuse to collide with a server launched outside systemd.
                    try:
                        _get(TENSOR_HEALTH, timeout=3)
                    except Exception:
                        pass
                    else:
                        raise RuntimeError(
                            "Unmanaged llama.cpp server is occupying port 11501. "
                            "Stop its external launcher; do not start a second server."
                        )

                    _service("start")
                    started_here = True

                _wait_tensor(True)
                yield

            else:
                # ComfyUI receives the GPUs only after the managed server exits.
                _service("stop")
                _wait_tensor(False)
                _wait_comfy_idle()
                yield

        finally:
            if mode == "tensor" and started_here:
                try:
                    _service("stop")
                    _wait_tensor(False)
                except Exception as e:
                    print("[DREAD GPU] Tensor shutdown warning:", e, flush=True)

            elif mode == "comfy":
                try:
                    _wait_comfy_idle(timeout=3600)
                    _post(
                        COMFY + "/free",
                        {"unload_models": True, "free_memory": True},
                        timeout=120
                    )
                except Exception as e:
                    print("[DREAD GPU] ComfyUI cleanup warning:", e, flush=True)

            fcntl.flock(lockfile.fileno(), fcntl.LOCK_UN)
