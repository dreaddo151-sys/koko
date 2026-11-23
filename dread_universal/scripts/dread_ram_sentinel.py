#!/usr/bin/env python3
"""DREAD RAM Sentinel — userspace earlyoom without root.

Watches MemAvailable. Before the kernel memory daemons jam (the freeze mode
observed on this machine on 2026-11-23: 7 unclean boots, kswapd/kcompactd soft
lockups, kernel page-fault BUGs, OOM killer never reached), it escalates:

  TIER 1 (< WARN MiB): kill disposable user junk (apport, evolution factories,
                      any chrome matching DREAD audit profiles).
  TIER 2 (< CRIT MiB): additionally SIGKILL the largest NON-protected process
                      (never Basalt/Qwen — the model is sacred per user policy;
                      never the desktop/kernel stack, never this process).
  REARM: after any kill it waits for 90 s of recovery (<WARN) before acting again.

Every kill is logged to stderr (journalctl --user -u dread-ram-sentinel).
"""
import os, re, time, signal, sys

WARN = int(os.environ.get("DREAD_RAM_WARN_MIB", 6144))     # start cleaning junk
CRIT = int(os.environ.get("DREAD_RAM_CRIT_MIB", 3072))     # kill biggest offender
INTERVAL = 3.0
REARM_OK_SECONDS = 90

# Never touch these — Basalt/Qwen are PROTECTED (user mandate 2026-11-23).
PROTECT = re.compile(
    r"basalt|serve\.server|/usr/bin/gnome-shell|gnome-session|/usr/sbin/gdm"
    r"|/usr/bin/dbus-daemon|pipewire|wireplumber|systemd|Xorg|wayland"
    r"|ram_sentinel|dsh-subprocess-local/lib/runner|hermes|dread-universal"
    r"|dread_gpu_supervisor_bridge|gateway|MainThread node.*dsh|node .*dread"
    r"|NetworkManager|ModemManager|bluez|accounts-daemon|logind"
    r"|gvfs|xdg-desktop-portal|gjs|ibus|im-launch|gsd-",
    re.I,
)
# Tier-1 disposable junk.
JUNK = re.compile(r"apport|ubuntu-bug|evolution-(addressbook|calendar|source)|gnome-software|update-notifier|whoops|checkbox", re.I)


def mem_avail_mib():
    with open("/proc/meminfo") as f:
        for line in f:
            if line.startswith("MemAvailable"):
                return int(line.split()[1]) // 1024
    return 999999


def procs():
    """yield (pid, rss_kib, cmdline) for user-visible processes."""
    me = os.getpid()
    for pid in os.listdir("/proc"):
        if not pid.isdigit() or int(pid) == me or int(pid) == 1:
            continue
        try:
            with open(f"/proc/{pid}/stat") as f:
                raw = f.read()
            # comm can contain spaces/parens — parse fields after the last ')'
            st = raw[raw.rfind(")") + 2:].split()
            if st[0] in ("Z", "X"):  # state
                continue
            rss = 0
            with open(f"/proc/{pid}/status") as f:
                for line in f:
                    if line.startswith("VmRSS"):
                        rss = int(line.split()[1])
                        break
            with open(f"/proc/{pid}/cmdline", "rb") as f:
                cmd = f.read().replace(b"\0", b" ").decode("utf-8", "replace")[:400].strip()
            if not cmd or PROTECT.search(cmd):
                continue
            yield int(pid), rss, cmd
        except (FileNotFoundError, PermissionError, ProcessLookupError, IndexError):
            continue


def kill(p, why):
    ts = time.strftime("%H:%M:%S")
    print(
        f"[{ts}] KILL({why}) pid={p[0]} rss={p[1] // 1024}M cmd={p[2][:160]}",
        flush=True,
    )
    try:
        os.kill(p[0], signal.SIGKILL)
        return True
    except OSError:
        return False


def main():
    print(
        f"RAM sentinel up: warn={WARN}M crit={CRIT}M (Basalt/Qwen PROTECTED)",
        flush=True,
    )
    last_action = 0.0
    ok_since = None
    while True:
        avail = mem_avail_mib()
        now = time.monotonic()
        # rearm after sustained recovery
        if avail >= WARN:
            if ok_since is None:
                ok_since = now
            elif now - ok_since > REARM_OK_SECONDS:
                if last_action:
                    print(f"[{time.strftime('%H:%M:%S')}] re-armed (avail {avail}M)", flush=True)
                    last_action = 0.0
                ok_since = None
        else:
            ok_since = None
        if avail >= WARN or now - last_action < 30:
            time.sleep(INTERVAL)
            continue
        last_action = now
        victims = list(procs())
        junk = [p for p in victims if JUNK.search(p[2])]
        for v in sorted(junk, key=lambda x: -x[1]):
            kill(v, "junk")
        if avail < CRIT:
            rest = [p for p in victims if not JUNK.search(p[2]) and p[1] > 150 * 1024]
            rest.sort(key=lambda x: -x[1])
            for v in rest[:2]:  # the two biggest offenders, never Basalt
                kill(v, "critical")
        time.sleep(INTERVAL)


if __name__ == "__main__":
    sys.exit(main())
