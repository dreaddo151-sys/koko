
#!/usr/bin/env python3
"""Run DREAD GPU scripts under the shared dual-GPU lease."""
import runpy, sys
from pathlib import Path
from dread_gpu_supervisor import gpu_lease

ROOT = Path(__file__).resolve().parent
if len(sys.argv) < 2:
    raise SystemExit("Usage: dread_gpu_exec.py SCRIPT [ARGS...]")

script = Path(sys.argv[1]).resolve()
if script.parent != ROOT or not script.is_file():
    raise SystemExit("Script must be an existing file in the DREAD workspace.")

sys.argv = [str(script)] + sys.argv[2:]
if script.name in ("tensor_pipeline_gpu1.py", "video_pipeline_gpu1.py"):
    with gpu_lease("comfy"):
        runpy.run_path(str(script), run_name="__main__")
else:
    runpy.run_path(str(script), run_name="__main__")
