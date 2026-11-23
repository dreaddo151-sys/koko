import json
import sys
import time
import uuid
import urllib.request
import urllib.error
import urllib.parse
import subprocess
import threading
import shutil
from pathlib import Path

ROOT = Path("/mnt/sabrent/dread_workspace")
COMFY = "http://127.0.0.1:8188"
DEST = ROOT / "outputs" / "latest_render.mp4"

def request(path, data=None, timeout=60):
    url = COMFY + path
    if data is None:
        req = urllib.request.Request(url)
    else:
        body = json.dumps(data).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=body,
            headers={"Content-Type": "application/json"}
        )
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return response.read()

def listen_comfy_progress(client_id, stop_event):
    """Forward live sampler events without closing on ComfyUI's long decode pauses."""
    try:
        from websockets.sync.client import connect
    except Exception as exc:
        print("[DREAD] Live ComfyUI progress unavailable (websockets import: %s)." % exc, flush=True)
        return

    url = COMFY.replace("http://", "ws://", 1) + "/ws?clientId=" + urllib.parse.quote(client_id)
    last_warning = 0.0
    while not stop_event.is_set():
        try:
            # ComfyUI can spend minutes in H3 VAE decode/encoding without
            # servicing WebSocket pings. Disable client keepalive so that a
            # quiet but valid render is not mistaken for a dead job.
            with connect(
                url,
                open_timeout=5,
                close_timeout=2,
                max_size=2**20,
                ping_interval=None,
            ) as ws:
                while not stop_event.is_set():
                    try:
                        message = ws.recv(timeout=1)
                    except TimeoutError:
                        continue
                    if not isinstance(message, str):
                        continue
                    try:
                        event = json.loads(message)
                    except (TypeError, ValueError):
                        continue
                    if event.get("type") != "progress":
                        continue
                    data = event.get("data") or {}
                    value, maximum = data.get("value"), data.get("max")
                    if not (
                        isinstance(value, (int, float))
                        and isinstance(maximum, (int, float))
                        and maximum > 0
                    ):
                        continue
                    node_id = data.get("node", "?")
                    print("[DREAD PROGRESS] %s/%s node=%s" % (value, maximum, node_id), flush=True)
                    if value >= maximum:
                        print(
                            "[DREAD STAGE] H3 decoding, audio processing, and video save are still pending.",
                            flush=True,
                        )
            if not stop_event.is_set():
                raise ConnectionError("ComfyUI progress connection closed.")
        except Exception as exc:
            if stop_event.is_set():
                break
            now = time.time()
            if now - last_warning >= 30:
                print(
                    "[DREAD WARNING] Live progress feed paused (%s); ComfyUI history polling will continue."
                    % str(exc)[:240],
                    flush=True,
                )
                last_warning = now
            stop_event.wait(2)


def node(class_type, inputs):
    return {"class_type": class_type, "inputs": inputs}

def run_comfy_video_pipeline(prompt, flashvsr_enabled=False):
    print("[DREAD] Connecting to ComfyUI...", flush=True)

    info = json.loads(request("/object_info"))
    required = [
        "UNETLoader", "CLIPLoader", "VAELoader",
        "MiniMaxH3ImageToVideo", "H3MSGPUSet", "H3MultiStream",
        "RandomNoise", "KSamplerSelect", "BasicScheduler",
        "BasicGuider", "SamplerCustomAdvanced", "VAEDecode",
        "VAEDecodeAudio", "CreateVideo", "SaveVideo"
    ]
    missing = [name for name in required if name not in info]
    if missing:
        raise RuntimeError("Missing ComfyUI nodes: " + ", ".join(missing))

    DEST.parent.mkdir(parents=True, exist_ok=True)
    workflow = {
        "1": node("UNETLoader", {
            "unet_name": "minimax_h3_fl2va_pruned_int8_convrot.safetensors",
            "weight_dtype": "default"
        }),
        "2": node("H3MSGPUSet", {
            "gpus": "0,1",
            "exclude": "",
            "shares": "",
            "min_free_vram_gb": 1.0,
            "max_gpus": 2
        }),
        "3": node("H3MultiStream", {
            "model": ["1", 0],
            "enabled": True,
            "second_gpu": -1,
            "exchange": "host",
            "exchange_chunks": 8,
            "sparse_attention": False,
            "dynamic_vram": "keep",
            "vram_block_cache": False,
            "vram_reserve_gb": 2.0,
            "sparse_vsa": False,
            "weight_cache": False,
            "cache_ram_reserve_gb": 8.0,
            "gpus": ["2", 0]
        }),
        "4": node("CLIPLoader", {
            "clip_name": "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
            "type": "minimax"
        }),
        "5": node("VAELoader", {
            "vae_name": "minimax_h3_video_vae_int8_convrot.safetensors"
        }),
        "6": node("VAELoader", {
            "vae_name": "minimax_h3_audio_vae_fp32.safetensors"
        }),
        "7": node("MiniMaxH3ImageToVideo", {
            "clip": ["4", 0],
            "vae": ["5", 0],
            "prompt": prompt,
            "width": 1344,
            "height": 768,
            "length": 124
        }),
        "8": node("RandomNoise", {
            "noise_seed": int(time.time()) % 2**32
        }),
        "9": node("KSamplerSelect", {
            "sampler_name": "res_multistep"
        }),
        "10": node("BasicScheduler", {
            "model": ["3", 0],
            "scheduler": "simple",
            "steps": 20,
            "denoise": 1.0
        }),
        "11": node("BasicGuider", {
            "model": ["3", 0],
            "conditioning": ["7", 0]
        }),
        "12": node("SamplerCustomAdvanced", {
            "noise": ["8", 0],
            "guider": ["11", 0],
            "sampler": ["9", 0],
            "sigmas": ["10", 0],
            "latent_image": ["7", 1]
        }),
        "13": node("VAEDecode", {
            "samples": ["12", 0],
            "vae": ["5", 0]
        }),
        "14": node("VAEDecodeAudio", {
            "samples": ["12", 0],
            "vae": ["6", 0]
        }),
        "15": node("CreateVideo", {
            "images": ["13", 0],
            "audio": ["14", 0],
            "fps": 24.0
        }),
        "16": node("SaveVideo", {
            "video": ["15", 0],
            "filename_prefix": "dread_h3",
            "format": "mp4"
        })
    }

    print("[DREAD] Submitting MiniMax H3 workflow.", flush=True)
    print("[DREAD] GPUs 0,1 | 1344x768 | 124 frames | 20 steps", flush=True)

    client_id = str(uuid.uuid4())
    progress_stop = threading.Event()
    progress_thread = threading.Thread(
        target=listen_comfy_progress,
        args=(client_id, progress_stop),
        daemon=True
    )
    progress_thread.start()

    try:
        result = json.loads(request("/prompt", {
            "prompt": workflow,
            "client_id": client_id
        }))

        if result.get("error"):
            raise RuntimeError("ComfyUI rejected workflow: " + json.dumps(result["error"]))

        prompt_id = result.get("prompt_id")
        if not prompt_id:
            raise RuntimeError("ComfyUI did not return a prompt ID: " + json.dumps(result))
    except Exception:
        progress_stop.set()
        progress_thread.join(timeout=2)
        raise

    print("[DREAD] Job ID: " + prompt_id, flush=True)
    print("[DREAD] Rendering. This can take around 18 minutes.", flush=True)

    deadline = time.time() + 1800
    completed_entry = None
    last_wait_notice = 0.0

    try:
        while time.time() < deadline:
            time.sleep(5)
            try:
                history = json.loads(request("/history/" + prompt_id, timeout=10))
            except urllib.error.HTTPError as exc:
                if exc.code not in (408, 429) and exc.code < 500:
                    raise RuntimeError("ComfyUI history returned HTTP %s." % exc.code) from exc
                history = None
            except (TimeoutError, urllib.error.URLError, ConnectionError, OSError) as exc:
                history = None
                now = time.time()
                if now - last_wait_notice >= 20:
                    print(
                        "[DREAD] ComfyUI status is temporarily slow during generation/decode; retrying. (%s)"
                        % str(exc)[:240],
                        flush=True,
                    )
                    last_wait_notice = now

            if history is None:
                continue
            entry = history.get(prompt_id)

            if not entry:
                now = time.time()
                if now - last_wait_notice >= 30:
                    print("[DREAD] No new ComfyUI history entry yet; the render remains active.", flush=True)
                    last_wait_notice = now
                continue

            status = entry.get("status", {})
            if status.get("status_str") == "error":
                raise RuntimeError(
                    "ComfyUI generation failed: " +
                    json.dumps(status.get("messages", status))[:8000]
                )

            if status.get("completed"):
                completed_entry = entry
                break
            now = time.time()
            if now - last_wait_notice >= 30:
                print(
                    "[DREAD] ComfyUI has no new progress event yet; continuing to check job history.",
                    flush=True,
                )
                last_wait_notice = now

    finally:
        progress_stop.set()
        progress_thread.join(timeout=2)

    if completed_entry is None:
        raise TimeoutError("Video generation exceeded the 30-minute limit.")

    entry = completed_entry

    if entry.get("status", {}).get("status_str") == "error":
        raise RuntimeError("Generation failed: " + json.dumps(entry)[:8000])

    saved = []
    for output in entry.get("outputs", {}).values():
        for key in ("images", "videos", "gifs"):
            saved.extend(output.get(key, []))

    if not saved:
        raise RuntimeError("Job completed but ComfyUI reported no saved video.")

    video = next(
        (item for item in saved if item.get("filename", "").lower().endswith(".mp4")),
        None
    )
    if not video:
        raise RuntimeError("No MP4 was found in ComfyUI outputs.")

    query = urllib.parse.urlencode({
        "filename": video["filename"],
        "subfolder": video.get("subfolder", ""),
        "type": video.get("type", "output")
    })

    print("[DREAD] Downloading completed MP4...", flush=True)
    video_data = request("/view?" + query, timeout=180)

    temp = DEST.with_suffix(".mp4.tmp")
    temp.write_bytes(video_data)

    if temp.stat().st_size < 100_000:
        temp.unlink(missing_ok=True)
        raise RuntimeError("Downloaded MP4 is unexpectedly small.")

    temp.replace(DEST)

    print("[DREAD] VIDEO GENERATION SUCCESSFUL", flush=True)
    print("[DREAD] Saved: " + str(DEST), flush=True)
    print("[DREAD] Size: %.2f MB" % (DEST.stat().st_size / 1024 / 1024), flush=True)

    if flashvsr_enabled:
        try:
            run_flashvsr_postprocess()
        except Exception as exc:
            # Keep the already-rendered H3 MP4 as a successful fallback.
            print("[DREAD WARNING] FlashVSR post-processing failed; keeping original H3 MP4.", flush=True)
            print("[DREAD WARNING] " + str(exc), flush=True)




def run_ltx25_video_pipeline(prompt, model="LTX-2.5 (Local GPU)",
                             resolution="960x544", duration="5", fps="24"):
    """Run the locally installed LTX-2.5 two-stage ComfyUI API workflow."""
    import copy

    workflow_path = ROOT / "outputs" / "ltx25_local_api.json"
    if not workflow_path.is_file():
        raise RuntimeError("Local LTX-2.5 API workflow not found: " + str(workflow_path))

    print("[DREAD] Preparing LOCAL LTX-2.5 workflow...", flush=True)
    info = json.loads(request("/object_info"))
    workflow = copy.deepcopy(json.loads(workflow_path.read_text()))

    # Inject the requested prompt and conservative initial render settings.
    try:
        workflow["5508"]["inputs"]["value"] = prompt
        workflow["5509"]["inputs"]["value"] = ""
        workflow["5511"]["inputs"]["value"] = float(fps)
        workflow["5512"]["inputs"]["value"] = float(duration)

        width, height = (int(x) for x in resolution.lower().split("x", 1))
        workflow["5514:3059"]["inputs"]["width"] = width
        workflow["5514:3059"]["inputs"]["height"] = height
        workflow["4852"]["inputs"]["filename_prefix"] = "dread_ltx25"
        workflow["5014:2483"]["inputs"]["text"] = ["5508", 0]
    except (KeyError, ValueError, TypeError) as exc:
        raise RuntimeError("The exported LTX-2.5 workflow has an unexpected node layout: " + str(exc))

    if width < 64 or height < 64 or width % 32 or height % 32:
        raise RuntimeError("LTX-2.5 dimensions must be at least 64 and divisible by 32.")
    if float(duration) <= 0 or float(fps) <= 0:
        raise RuntimeError("LTX-2.5 duration and FPS must be positive.")

    # Route positive/negative conditioning directly through the local
    # LTXVConditioning node. Bypass optional cloud API and prompt-enhancer paths.
    replacements = {
        ("5014:5556", 0): ("5508", 0),
        ("5014:5558", 0): ("5014:1241", 0),
        ("5014:5560", 0): ("5014:1241", 1),
    }

    def rewrite_links(value):
        if isinstance(value, list):
            if (len(value) == 2 and isinstance(value[0], str)
                    and isinstance(value[1], int)):
                target = replacements.get((value[0], value[1]))
                return list(target) if target else value
            return [rewrite_links(item) for item in value]
        if isinstance(value, dict):
            return {key: rewrite_links(item) for key, item in value.items()}
        return value

    workflow = rewrite_links(workflow)

    # Remove the now-bypassed optional enhancer, image-to-video and API branches.
    bypassed = {
        "5014:5556", "5014:5555", "5014:5549", "5014:5546",
        "5575:5572", "5014:5504", "5014:5505", "5014:5557",
        "5014:5558", "5014:5560",
    }
    for node_id in bypassed:
        workflow.pop(node_id, None)

    # Keep only nodes reachable from SaveVideo, following actual API links.
    if "4852" not in workflow:
        raise RuntimeError("SaveVideo node 4852 is missing from the exported workflow.")

    reachable = set()

    def visit(node_id):
        if node_id in reachable:
            return
        current = workflow.get(node_id)
        if not isinstance(current, dict) or "class_type" not in current:
            raise RuntimeError("Workflow link refers to missing node: " + str(node_id))
        reachable.add(node_id)

        def follow(value):
            if (isinstance(value, list) and len(value) == 2
                    and isinstance(value[0], str) and isinstance(value[1], int)):
                if value[0] in workflow:
                    visit(value[0])
            elif isinstance(value, list):
                for item in value:
                    follow(item)
            elif isinstance(value, dict):
                for item in value.values():
                    follow(item)

        for value in current.get("inputs", {}).values():
            follow(value)

    visit("4852")
    workflow = {key: value for key, value in workflow.items() if key in reachable}

    # Confirm every remaining node is registered and every required model
    # filename is visible to this ComfyUI instance before queueing anything.
    missing_nodes = sorted({
        node_data["class_type"]
        for node_data in workflow.values()
        if node_data["class_type"] not in info
    })
    if missing_nodes:
        raise RuntimeError("ComfyUI is missing local LTX workflow nodes: " + ", ".join(missing_nodes))

    loader_keys = {
        "UNETLoader": "unet_name",
        "CLIPLoader": "clip_name",
        "VAELoader": "vae_name",
        "LatentUpscaleModelLoader": "model_name",
    }
    for node_id, node_data in workflow.items():
        cls = node_data["class_type"]
        key = loader_keys.get(cls)
        if not key:
            continue
        filename = node_data.get("inputs", {}).get(key)
        schema = info.get(cls, {}).get("input", {})
        spec = schema.get("required", {}).get(key)
        if spec is None:
            spec = schema.get("optional", {}).get(key)
        choices = spec[0] if (
            isinstance(spec, (list, tuple)) and spec
            and isinstance(spec[0], (list, tuple))
        ) else None
        if choices is not None and filename not in choices:
            raise RuntimeError(
                "ComfyUI cannot see model '%s' required by node %s (%s). "
                "The model is on disk but may not be registered in ComfyUI's model paths."
                % (filename, node_id, cls)
            )

    DEST.parent.mkdir(parents=True, exist_ok=True)
    print("[DREAD] Local model graph ready: %s nodes." % len(workflow), flush=True)
    print("[DREAD] Settings: %s | %ss | %s FPS." %
          (resolution, duration, fps), flush=True)
    print("[DREAD] Submitting local LTX-2.5 job to ComfyUI...", flush=True)

    client_id = str(uuid.uuid4())
    progress_stop = threading.Event()
    progress_thread = threading.Thread(
        target=listen_comfy_progress,
        args=(client_id, progress_stop),
        daemon=True,
    )
    progress_thread.start()

    try:
        try:
            result = json.loads(request("/prompt", {
                "prompt": workflow,
                "client_id": client_id,
            }))
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace")
            raise RuntimeError(
                "ComfyUI rejected the local LTX workflow (HTTP %s): %s"
                % (exc.code, detail[:3000])
            ) from exc

        if result.get("error"):
            raise RuntimeError("ComfyUI rejected local LTX workflow: " +
                               json.dumps(result["error"])[:3000])
        prompt_id = result.get("prompt_id")
        if not prompt_id:
            raise RuntimeError("ComfyUI returned no prompt ID: " + json.dumps(result)[:2000])

        print("[DREAD] Local LTX-2.5 job ID: " + prompt_id, flush=True)
        deadline = time.time() + 1800
        last_notice = 0.0

        while time.time() < deadline:
            time.sleep(5)
            try:
                history = json.loads(request("/history/" + prompt_id, timeout=15))
            except urllib.error.HTTPError as exc:
                if exc.code not in (408, 429) and exc.code < 500:
                    raise RuntimeError("ComfyUI history returned HTTP %s." % exc.code) from exc
                continue
            except (TimeoutError, urllib.error.URLError, ConnectionError, OSError) as exc:
                now = time.time()
                if now - last_notice >= 30:
                    print("[DREAD] Waiting for ComfyUI history response: %s" %
                          str(exc)[:200], flush=True)
                    last_notice = now
                continue

            entry = history.get(prompt_id)
            if not entry:
                continue

            for node_output in entry.get("outputs", {}).values():
                # SaveVideo can return MP4 files under "images" with
                # metadata.kind="video", rather than under "videos".
                video_items = list(node_output.get("videos") or [])
                for item in node_output.get("images") or []:
                    metadata = item.get("metadata") or {}
                    filename_hint = str(item.get("filename", "")).lower()
                    if metadata.get("kind") == "video" or filename_hint.endswith(".mp4"):
                        video_items.append(item)

                for video in video_items:
                    filename = video.get("filename")
                    subfolder = video.get("subfolder", "")
                    fullpath = video.get("fullpath")

                    if fullpath and Path(fullpath).is_file():
                        temp = DEST.with_name(DEST.stem + ".ltx_tmp.mp4")
                        shutil.copy2(fullpath, temp)
                    elif filename:
                        query = urllib.parse.urlencode({
                            "filename": filename,
                            "subfolder": subfolder,
                        })
                        data = request("/view?" + query, timeout=180)
                        temp = DEST.with_name(DEST.stem + ".ltx_tmp.mp4")
                        temp.write_bytes(data)
                    else:
                        continue

                    if not temp.is_file() or temp.stat().st_size < 100_000:
                        temp.unlink(missing_ok=True)
                        raise RuntimeError("Local LTX output MP4 is missing or unexpectedly small.")

                    temp.replace(DEST)
                    print("[DREAD] LOCAL LTX-2.5 VIDEO GENERATION SUCCESSFUL", flush=True)
                    print("[DREAD] Saved: " + str(DEST), flush=True)
                    print("[DREAD] Size: %.2f MB" %
                          (DEST.stat().st_size / 1024 / 1024), flush=True)
                    return

            status = entry.get("status", {})
            if status.get("status_str") == "error":
                raise RuntimeError("Local LTX-2.5 job failed: " +
                                   json.dumps(status.get("messages", []))[:3000])

    finally:
        progress_stop.set()
        progress_thread.join(timeout=2)

    raise TimeoutError("Local LTX-2.5 pipeline exceeded the 30-minute limit.")

def run_flashvsr_postprocess():
    """Optional additive FlashVSR upscale; preserve H3 output/audio on failure."""
    print("[DREAD] FlashVSR Full 2x post-processing enabled.", flush=True)
    comfy_root = ROOT / "ComfyUI"
    cli = comfy_root / "custom_nodes" / "ComfyUI-FlashVSR" / "cli_main.py"
    models_dir = comfy_root / "models"

    # Strict local-only preflight: never start FlashVSR if required weights are absent.
    model_dir = models_dir / "FlashVSR-v1.1"
    required_weights = [
        model_dir / "diffusion_pytorch_model_streaming_dmd.safetensors",
        model_dir / "LQ_proj_in.ckpt",
        model_dir / "Wan2.1_VAE.pth",
    ]
    missing_weights = [str(path) for path in required_weights if not path.is_file()]
    if missing_weights:
        raise RuntimeError(
            "FlashVSR local model preflight failed; no download was attempted. Missing: "
            + ", ".join(missing_weights)
        )

    python_candidates = [comfy_root / ".venv" / "bin" / "python", ROOT / "dread_env" / "bin" / "python3"]
    flash_python = next((p for p in python_candidates if p.is_file()), None)
    if not cli.is_file():
        raise RuntimeError("FlashVSR CLI not found: " + str(cli))
    if flash_python is None:
        raise RuntimeError("Could not find ComfyUI Python venv for FlashVSR.")

    intermediate = DEST.with_name("latest_render_flashvsr_2x_video_only.mp4")
    final_temp = DEST.with_name("latest_render_flashvsr_final.mp4")
    intermediate.unlink(missing_ok=True)
    final_temp.unlink(missing_ok=True)

    command = [
        str(flash_python), str(cli),
        "--input", str(DEST),
        "--output", str(intermediate),
        "--model", "FlashVSR-v1.1",
        "--mode", "full",
        "--vae_model", "Wan2.1",
        "--scale", "2",
        "--precision", "auto",
        "--device", "cuda:1",
        "--attention_mode", "auto",
        "--models_dir", str(models_dir),
        "--codec", "libx264",
        "--crf", "18",
    ]
    print("[DREAD] Running FlashVSR CLI on GPU 1 (2x upscale, offload enabled)...", flush=True)
    proc = subprocess.run(command, cwd=str(cli.parent), capture_output=True, text=True, timeout=7200)
    log = (proc.stdout or "") + ("\n--- STDERR ---\n" + proc.stderr if proc.stderr else "")
    if log:
        print(log[-12000:], flush=True)
    if proc.returncode != 0 or not intermediate.is_file() or intermediate.stat().st_size < 100_000:
        intermediate.unlink(missing_ok=True)
        raise RuntimeError("FlashVSR failed; original H3 MP4 remains available. See FlashVSR log above.")

    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        try:
            import imageio_ffmpeg
            ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
        except Exception:
            ffmpeg = None
    if not ffmpeg:
        intermediate.unlink(missing_ok=True)
        raise RuntimeError("FFmpeg unavailable for final audio mux; original H3 MP4 remains available.")

    # Preserve FlashVSR's native output dimensions and video quality.
    # Remux the original H3 audio without resizing or re-encoding the video.
    mux = [
        ffmpeg, "-y", "-hide_banner", "-loglevel", "error",
        "-i", str(intermediate), "-i", str(DEST),
        "-map", "0:v:0", "-map", "1:a?",
        "-c:v", "copy",
        "-c:a", "aac", "-b:a", "192k",
        "-shortest", "-movflags", "+faststart",
        str(final_temp)
    ]
    mux_proc = subprocess.run(mux, capture_output=True, text=True, timeout=1800)
    if mux_proc.returncode != 0 or not final_temp.is_file() or final_temp.stat().st_size < 100_000:
        final_temp.unlink(missing_ok=True)
        intermediate.unlink(missing_ok=True)
        raise RuntimeError("FlashVSR final audio mux failed; original H3 MP4 remains available: " + (mux_proc.stderr or "unknown ffmpeg error")[-4000:])

    final_temp.replace(DEST)
    intermediate.unlink(missing_ok=True)
    print("[DREAD] FlashVSR post-process complete: native output resolution preserved; H3 audio remuxed.", flush=True)
    print("[DREAD] Final MP4: " + str(DEST), flush=True)

if __name__ == "__main__":
    args = sys.argv[1:]

    if any(arg in ("-h", "--help") for arg in args):
        print("Usage: python video_pipeline_gpu1.py [--flashvsr] [--ltx25] <video prompt>")
        print("FlashVSR is optional. H3 generation is not started by --help.")
        sys.exit(0)

    flashvsr_enabled = "--flashvsr" in args
    args = [arg for arg in args if arg != "--flashvsr"]
    ltx25_enabled = "--ltx25" in args
    args = [arg for arg in args if arg != "--ltx25"]

    if any(arg.startswith("-") for arg in args):
        print("[DREAD ERROR] Unknown command-line option. No video job submitted: "
              + " ".join(arg for arg in args if arg.startswith("-")),
              file=sys.stderr, flush=True)
        sys.exit(2)

    prompt = " ".join(args).strip()
    if not prompt:
        prompt = (
            "A cinematic photorealistic shot of a red vintage sports car "
            "driving along a coastal road at sunset, ocean waves beside "
            "the road, smooth tracking camera and realistic motion."
        )

    try:
        if ltx25_enabled:
            run_ltx25_video_pipeline(prompt)
        else:
            run_comfy_video_pipeline(prompt, flashvsr_enabled=flashvsr_enabled)
    except Exception as exc:
        print("[DREAD ERROR] " + str(exc), file=sys.stderr, flush=True)
        sys.exit(1)
