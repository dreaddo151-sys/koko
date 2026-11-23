import subprocess
import json
import sys

def run_audit():
    print("[*] Running local Semgrep security audit on workspace...")
    try:
        # Run semgrep on the workspace directory using r2c rules or local rules
        cmd = ["semgrep", "scan", "--config", "auto", "--json", "."]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        
        if result.returncode in [0, 1]: # Semgrep returns 1 if findings are found
            print(result.stdout)
        else:
            print(f"Semgrep error: {result.stderr}", file=sys.stderr)
    except Exception as e:
        print(f"Execution failed: {e}", file=sys.stderr)

if __name__ == "__main__":
    run_audit()
