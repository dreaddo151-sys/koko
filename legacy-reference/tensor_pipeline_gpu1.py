import torch
import os
import sys
import re
from diffusers import StableDiffusionXLPipeline

def run_sdxl_pipeline():
    print("[*] Initializing SDXL pipeline")
    device = "cuda:1" if torch.cuda.device_count() > 1 else "cuda:0"
    print(f"[+] Device: {torch.cuda.get_device_name(torch.device(device))}")

    # Usage: python tensor_pipeline_gpu1.py "prompt" [size] [steps]
    raw_prompt = "cinematic photorealistic scene"
    if len(sys.argv) > 1:
        raw_prompt = sys.argv[1]
    elif os.path.exists("outputs/latest_prompt.txt"):
        with open("outputs/latest_prompt.txt", "r", encoding="utf-8") as f:
            raw_prompt = f.read().strip() or raw_prompt

    size = int(sys.argv[2]) if len(sys.argv) > 2 else 1024
    steps = int(sys.argv[3]) if len(sys.argv) > 3 else 30

    # SDXL dimensions should be multiples of 64.
    size = max(768, min(size, 1536))
    size = (size // 64) * 64
    steps = max(15, min(steps, 60))

    cleaned = raw_prompt.strip()
    for phrase in ["show me", "create an image of", "generate an image of",
                   "create", "generate", "show me", "images", "image",
                   "photos", "photo", "pictures", "picture"]:
        cleaned = re.sub(re.escape(phrase), "", cleaned, flags=re.IGNORECASE)
    cleaned = re.sub(r"\s+", " ", cleaned).strip(" ,.")

    enhanced_prompt = (
        f"{cleaned}, cinematic composition, natural lighting, "
        "high detail, realistic materials, sharp focus, professional photography"
    )

    print(f"[+] Prompt: {enhanced_prompt}")
    print(f"[+] Output size: {size}x{size}; steps: {steps}")
    print("[+] Loading SDXL model...")

    pipe = StableDiffusionXLPipeline.from_pretrained(
        "stabilityai/stable-diffusion-xl-base-1.0",
        torch_dtype=torch.float16,
        variant="fp16"
    ).to(device)

    pipe.enable_attention_slicing()

    print("[+] Running inference...")
    result = pipe(
        prompt=enhanced_prompt,
        negative_prompt="blurry, low quality, distorted, deformed, artifacts, text, watermark",
        height=size,
        width=size,
        num_inference_steps=steps,
        guidance_scale=7.0
    )

    os.makedirs("outputs", exist_ok=True)
    out_path = "outputs/latest_render.png"
    result.images[0].save(out_path)
    print(f"[+] Generated image: {out_path}")

if __name__ == "__main__":
    run_sdxl_pipeline()
