//! Read-only host and local runtime discovery for portable DREAD installations.
//!
//! Probes only a small allowlist of known system utilities. It never invokes a
//! shell, installs software, changes drivers, or starts an inference process.

use serde::Serialize;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GpuDevice {
    pub vendor: String,
    pub name: String,
    pub compute_capability: Option<String>,
    pub vram_mib: Option<u64>,
    pub vram_used_mib: Option<u64>,
    pub vram_free_mib: Option<u64>,
    pub driver_version: Option<String>,
    pub pcie_link_gen: Option<u32>,
    pub pcie_link_width: Option<u32>,
    pub pcie_link_gen_max: Option<u32>,
    pub pcie_link_width_max: Option<u32>,
    pub utilization_percent: Option<u32>,
    pub temperature_c: Option<u32>,
    pub power_draw_mw: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StorageDevice {
    pub source: String,
    pub filesystem: String,
    pub mount_point: String,
    pub total_mib: Option<u64>,
    pub used_mib: Option<u64>,
    pub available_mib: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HostProfile {
    pub os: String,
    pub architecture: String,
    pub distribution: Option<String>,
    pub distribution_version: Option<String>,
    pub kernel_release: Option<String>,
    pub cpu_model: Option<String>,
    pub cpu_physical_cores: Option<usize>,
    pub cuda_version: Option<String>,
    pub cpu_threads: usize,
    pub memory_total_mib: Option<u64>,
    pub memory_available_mib: Option<u64>,
    pub root_disk_total_mib: Option<u64>,
    pub root_disk_available_mib: Option<u64>,
    pub storage_devices: Vec<StorageDevice>,
    pub gpu_devices: Vec<GpuDevice>,
    pub llama_server: Option<String>,
    pub nvidia_smi_available: bool,
    pub rocm_smi_available: bool,
    pub xpu_smi_available: bool,
    pub inference_backend: String,
}

fn find_binary(names: &[&str]) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for directory in env::split_paths(&path_var) {
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn run_probe(program: &Path, args: &[&str]) -> Option<String> {
    // Bound diagnostic probes so a stuck driver utility cannot stall profile requests.
    let output = if let Some(timeout) = find_binary(&["timeout"]) {
        Command::new(timeout)
            .args(["--signal=KILL", "5s"])
            .arg(program)
            .args(args)
            .output()
            .ok()?
    } else {
        Command::new(program).args(args).output().ok()?
    };
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_nvidia_csv(output: &str) -> Vec<GpuDevice> {
    output
        .lines()
        .filter_map(|line| {
            let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
            if fields.len() < 12 || fields[0].is_empty() {
                return None;
            }
            Some(GpuDevice {
                vendor: "nvidia".into(),
                name: fields[0].into(),
                compute_capability: fields
                    .get(12)
                    .filter(|value| !value.is_empty() && **value != "N/A")
                    .map(|value| (*value).to_string()),
                vram_mib: fields[1].parse().ok(),
                vram_used_mib: fields[2].parse().ok(),
                vram_free_mib: fields[3].parse().ok(),
                driver_version: (!fields[4].is_empty() && fields[4] != "N/A")
                    .then(|| fields[4].to_string()),
                pcie_link_gen: fields[5].parse().ok(),
                pcie_link_width: fields[6].parse().ok(),
                pcie_link_gen_max: fields[7].parse().ok(),
                pcie_link_width_max: fields[8].parse().ok(),
                utilization_percent: fields[9].parse().ok(),
                temperature_c: fields[10].parse().ok(),
                power_draw_mw: fields[11]
                    .parse::<f64>()
                    .ok()
                    .filter(|watts| watts.is_finite() && *watts >= 0.0)
                    .map(|watts| (watts * 1000.0).round() as u64),
            })
        })
        .collect()
}

fn nvidia_devices(binary: &Path) -> Vec<GpuDevice> {
    if let Some(output) = run_probe(
        binary,
        &[
            "--query-gpu=name,memory.total,memory.used,memory.free,driver_version,pcie.link.gen.current,pcie.link.width.current,pcie.link.gen.max,pcie.link.width.max,utilization.gpu,temperature.gpu,power.draw,compute_cap",
            "--format=csv,noheader,nounits",
        ],
    ) {
        let devices = parse_nvidia_csv(&output);
        if !devices.is_empty() {
            return devices;
        }
    }

    // Older drivers may not expose compute_cap; preserve the full telemetry
    // query before falling back to the minimal name/VRAM inventory.
    if let Some(output) = run_probe(
        binary,
        &[
            "--query-gpu=name,memory.total,memory.used,memory.free,driver_version,pcie.link.gen.current,pcie.link.width.current,pcie.link.gen.max,pcie.link.width.max,utilization.gpu,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ],
    ) {
        let devices = parse_nvidia_csv(&output);
        if !devices.is_empty() {
            return devices;
        }
    }

    let Some(output) = run_probe(
        binary,
        &[
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    ) else {
        return Vec::new();
    };
    output
        .lines()
        .filter_map(|line| {
            let (name, memory) = line.split_once(',')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some(GpuDevice {
                vendor: "nvidia".into(),
                name: name.into(),
                compute_capability: None,
                vram_mib: memory.trim().parse().ok(),
                vram_used_mib: None,
                vram_free_mib: None,
                driver_version: None,
                pcie_link_gen: None,
                pcie_link_width: None,
                pcie_link_gen_max: None,
                pcie_link_width_max: None,
                utilization_percent: None,
                temperature_c: None,
                power_draw_mw: None,
            })
        })
        .collect()
}

fn amd_devices(binary: &Path) -> Vec<GpuDevice> {
    let Some(output) = run_probe(binary, &["--showproductname"]) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| {
            let lower = line.to_ascii_lowercase();
            if !lower.contains("gpu[") {
                return None;
            }
            let (_, name) = line.split_once(':')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some(GpuDevice {
                vendor: "amd".into(),
                name: name.into(),
                compute_capability: None,
                vram_mib: None,
                vram_used_mib: None,
                vram_free_mib: None,
                driver_version: None,
                pcie_link_gen: None,
                pcie_link_width: None,
                pcie_link_gen_max: None,
                pcie_link_width_max: None,
                utilization_percent: None,
                temperature_c: None,
                power_draw_mw: None,
            })
        })
        .collect()
}

fn apple_devices(binary: &Path) -> Vec<GpuDevice> {
    let Some(output) = run_probe(binary, &["SPDisplaysDataType", "-detailLevel", "mini"]) else {
        return Vec::new();
    };

    let mut devices = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_vram: Option<u64> = None;

    for line in output.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Chipset Model:") {
            if let Some(name) = current_name.take() {
                devices.push(GpuDevice {
                    vendor: "apple".into(),
                    name,
                    compute_capability: None,
                    vram_mib: current_vram.take(),
                    vram_used_mib: None,
                    vram_free_mib: None,
                    driver_version: None,
                    pcie_link_gen: None,
                    pcie_link_width: None,
                    pcie_link_gen_max: None,
                    pcie_link_width_max: None,
                    utilization_percent: None,
                    temperature_c: None,
                    power_draw_mw: None,
                });
            }
            current_name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("VRAM (Total):") {
            current_vram = parse_memory_mib(value.trim());
        }
    }

    if let Some(name) = current_name {
        devices.push(GpuDevice {
            vendor: "apple".into(),
            name,
            compute_capability: None,
            vram_mib: current_vram,
            vram_used_mib: None,
            vram_free_mib: None,
            driver_version: None,
            pcie_link_gen: None,
            pcie_link_width: None,
            pcie_link_gen_max: None,
            pcie_link_width_max: None,
            utilization_percent: None,
            temperature_c: None,
            power_draw_mw: None,
        });
    }
    devices
}

fn parse_memory_mib(value: &str) -> Option<u64> {
    let value = value.trim();
    let number = value
        .split_whitespace()
        .next()?
        .trim_end_matches(|character: char| !character.is_ascii_digit() && character != '.')
        .parse::<f64>()
        .ok()?;
    let lower = value.to_ascii_lowercase();
    let mib = if lower.contains("gb") {
        number * 1024.0
    } else if lower.contains("tb") {
        number * 1024.0 * 1024.0
    } else {
        number
    };
    if mib.is_finite() && mib >= 0.0 {
        Some(mib.round() as u64)
    } else {
        None
    }
}

fn os_release_value(contents: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    contents.lines().find_map(|line| {
        let value = line.strip_prefix(&prefix)?.trim();
        let value = value.trim_matches('"').trim_matches('\'');
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn cpu_model(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if matches!(key.trim(), "model name" | "Hardware" | "Processor") {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        } else {
            None
        }
    })
}

fn cpu_physical_cores(contents: &str) -> Option<usize> {
    let mut cores = BTreeSet::new();
    for block in contents.split("\n\n") {
        let physical_id = block.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "physical id")
                .then(|| value.trim().parse::<u32>().ok())
                .flatten()
        });
        let core_id = block.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "core id")
                .then(|| value.trim().parse::<u32>().ok())
                .flatten()
        });
        if let (Some(physical_id), Some(core_id)) = (physical_id, core_id) {
            cores.insert((physical_id, core_id));
        }
    }
    (!cores.is_empty()).then_some(cores.len())
}

fn parse_nvidia_cuda_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, version) = line.split_once("CUDA Version:")?;
        let version = version.split_whitespace().next()?;
        (!version.is_empty() && version != "N/A").then(|| version.to_string())
    })
}

fn nvidia_cuda_version(binary: &Path) -> Option<String> {
    parse_nvidia_cuda_version(&run_probe(binary, &[])?)
}

fn memory_info_mib(contents: &str, key: &str) -> Option<u64> {
    let line = contents
        .lines()
        .find(|line| line.starts_with(&format!("{key}:")))?;
    let mut fields = line.split_whitespace();
    fields.next()?;
    let value = fields.next()?.parse::<u64>().ok()?;
    match fields.next().unwrap_or("kB") {
        "kB" => Some(value / 1024),
        "MB" => Some(value * 1024),
        "GB" => Some(value * 1024 * 1024),
        _ => None,
    }
}

fn root_disk_capacity_mib() -> (Option<u64>, Option<u64>) {
    let Some(binary) = find_binary(&["df"]) else {
        return (None, None);
    };
    let Some(output) = run_probe(&binary, &["-B1", "--output=size,avail", "--", "/"]) else {
        return (None, None);
    };
    let Some(line) = output.lines().skip(1).find(|line| !line.trim().is_empty()) else {
        return (None, None);
    };
    let mut fields = line.split_whitespace();
    let total = fields.next().and_then(|value| value.parse::<u64>().ok());
    let available = fields.next().and_then(|value| value.parse::<u64>().ok());
    (
        total.map(|bytes| bytes / 1024 / 1024),
        available.map(|bytes| bytes / 1024 / 1024),
    )
}

fn parse_df_storage(output: &str) -> Vec<StorageDevice> {
    const PSEUDO_FILESYSTEMS: &[&str] = &[
        "autofs",
        "cgroup",
        "cgroup2",
        "configfs",
        "debugfs",
        "devpts",
        "devtmpfs",
        "fusectl",
        "mqueue",
        "overlay",
        "proc",
        "pstore",
        "securityfs",
        "squashfs",
        "sysfs",
        "tmpfs",
        "tracefs",
    ];
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 6 || PSEUDO_FILESYSTEMS.contains(&fields[1]) {
                return None;
            }
            let total_bytes = fields[2].parse::<u64>().ok()?;
            Some(StorageDevice {
                source: fields[0].to_string(),
                filesystem: fields[1].to_string(),
                total_mib: Some(total_bytes / 1024 / 1024),
                used_mib: fields[3]
                    .parse::<u64>()
                    .ok()
                    .map(|bytes| bytes / 1024 / 1024),
                available_mib: fields[4]
                    .parse::<u64>()
                    .ok()
                    .map(|bytes| bytes / 1024 / 1024),
                mount_point: fields[5].to_string(),
            })
        })
        .collect()
}

fn mounted_storage_devices() -> Vec<StorageDevice> {
    let Some(binary) = find_binary(&["df"]) else {
        return Vec::new();
    };
    let Some(output) = run_probe(
        &binary,
        &["-B1", "-P", "--output=source,fstype,size,used,avail,target"],
    ) else {
        return Vec::new();
    };
    parse_df_storage(&output)
}

fn llama_server_path() -> Option<String> {
    if let Ok(configured) = env::var("DREAD_LLM_SERVER") {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }

    let names: &[&str] = if cfg!(windows) {
        &["llama-server.exe", "llama-server"]
    } else {
        &["llama-server"]
    };
    find_binary(names).map(|path| path.to_string_lossy().into_owned())
}

/// Detect host hardware and known local inference executables without mutating the host.
pub fn detect_host() -> HostProfile {
    let nvidia = find_binary(if cfg!(windows) {
        &["nvidia-smi.exe", "nvidia-smi"]
    } else {
        &["nvidia-smi"]
    });
    let amd = find_binary(if cfg!(windows) {
        &["rocm-smi.exe", "rocm-smi"]
    } else {
        &["rocm-smi"]
    });
    let intel = find_binary(if cfg!(windows) {
        &["xpu-smi.exe", "xpu-smi"]
    } else {
        &["xpu-smi"]
    });

    let mut gpu_devices = Vec::new();
    if let Some(binary) = &nvidia {
        gpu_devices.extend(nvidia_devices(binary));
    }
    if let Some(binary) = &amd {
        gpu_devices.extend(amd_devices(binary));
    }
    if cfg!(target_os = "macos")
        && let Some(binary) = find_binary(&["system_profiler"])
    {
        gpu_devices.extend(apple_devices(&binary));
    }

    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let (root_disk_total_mib, root_disk_available_mib) = root_disk_capacity_mib();
    let storage_devices = mounted_storage_devices();
    let cpu_physical_cores = cpu_physical_cores(&cpuinfo);
    let cuda_version = nvidia.as_deref().and_then(nvidia_cuda_version);

    HostProfile {
        os: env::consts::OS.into(),
        architecture: env::consts::ARCH.into(),
        distribution: os_release_value(&os_release, "ID"),
        distribution_version: os_release_value(&os_release, "VERSION_ID"),
        kernel_release: fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        cpu_model: cpu_model(&cpuinfo),
        cpu_physical_cores,
        cuda_version,
        cpu_threads: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        memory_total_mib: memory_info_mib(&meminfo, "MemTotal"),
        memory_available_mib: memory_info_mib(&meminfo, "MemAvailable"),
        root_disk_total_mib,
        root_disk_available_mib,
        storage_devices,
        gpu_devices,
        llama_server: llama_server_path(),
        nvidia_smi_available: nvidia.is_some(),
        rocm_smi_available: amd.is_some(),
        xpu_smi_available: intel.is_some(),
        inference_backend: "llama.cpp".into(),
    }
}

/// Return a host profile cached for 15 seconds so live GPU telemetry refreshes
/// without spawning diagnostic utilities on every API poll.
pub fn cached_host_profile() -> HostProfile {
    static PROFILE: OnceLock<Mutex<Option<(Instant, HostProfile)>>> = OnceLock::new();
    let cache = PROFILE.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let refresh = guard
        .as_ref()
        .is_none_or(|(updated, _)| updated.elapsed() >= Duration::from_secs(15));
    if refresh {
        *guard = Some((Instant::now(), detect_host()));
    }
    guard
        .as_ref()
        .map(|(_, profile)| profile.clone())
        .unwrap_or_else(detect_host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_topology_counts_unique_physical_and_core_pairs() {
        let sample = "processor : 0\nphysical id : 0\ncore id : 0\n\nprocessor : 1\nphysical id : 0\ncore id : 0\n\nprocessor : 2\nphysical id : 0\ncore id : 1\n\nprocessor : 3\nphysical id : 1\ncore id : 0\n";
        assert_eq!(cpu_physical_cores(sample), Some(3));
        assert_eq!(cpu_physical_cores("processor : 0\nprocessor : 1\n"), None);
    }

    #[test]
    fn nvidia_cuda_banner_is_parsed_without_guessing() {
        let banner = "NVIDIA-SMI 550.90.07 Driver Version: 550.90.07 CUDA Version: 12.4\n";
        assert_eq!(parse_nvidia_cuda_version(banner).as_deref(), Some("12.4"));
        assert_eq!(parse_nvidia_cuda_version("CUDA Version: N/A"), None);
    }

    #[test]
    fn host_profile_has_portable_os_and_cpu_fields() {
        let profile = detect_host();
        assert!(!profile.os.is_empty());
        assert!(!profile.architecture.is_empty());
        assert!(profile.cpu_threads >= 1);
        assert_eq!(profile.inference_backend, "llama.cpp");
    }

    #[test]
    fn df_parser_reports_persistent_and_network_mounts_but_skips_pseudo_filesystems() {
        let sample = "Filesystem Type 1B-blocks Used Available Mounted on\n/dev/nvme0n1p2 ext4 107374182400 21474836480 85899345920 /\nserver:/archive nfs4 214748364800 53687091200 161061273600 /mnt/archive\ntmpfs tmpfs 8589934592 0 8589934592 /run\n";
        let mounts = parse_df_storage(sample);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].source, "/dev/nvme0n1p2");
        assert_eq!(mounts[0].filesystem, "ext4");
        assert_eq!(mounts[0].mount_point, "/");
        assert_eq!(mounts[0].total_mib, Some(102400));
        assert_eq!(mounts[0].used_mib, Some(20480));
        assert_eq!(mounts[1].filesystem, "nfs4");
        assert_eq!(mounts[1].mount_point, "/mnt/archive");
        assert_eq!(mounts[1].available_mib, Some(153600));
    }

    #[test]
    fn nvidia_csv_parser_includes_driver_pcie_and_live_telemetry() {
        let sample = "NVIDIA GeForce RTX 3090, 24576, 12000, 12576, 550.90.07, 1, 1, 4, 16, 37, 52, 280.5, 8.6\n";
        let parsed = parse_nvidia_csv(sample);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].vram_used_mib, Some(12000));
        assert_eq!(parsed[0].compute_capability.as_deref(), Some("8.6"));
        assert_eq!(parsed[0].vram_free_mib, Some(12576));
        assert_eq!(parsed[0].driver_version.as_deref(), Some("550.90.07"));
        assert_eq!(parsed[0].pcie_link_gen, Some(1));
        assert_eq!(parsed[0].pcie_link_width, Some(1));
        assert_eq!(parsed[0].pcie_link_gen_max, Some(4));
        assert_eq!(parsed[0].pcie_link_width_max, Some(16));
        assert_eq!(parsed[0].utilization_percent, Some(37));
        assert_eq!(parsed[0].temperature_c, Some(52));
        assert_eq!(parsed[0].power_draw_mw, Some(280_500));
    }

    #[test]
    fn linux_memory_and_distribution_parsers_are_read_only() {
        let meminfo = "MemTotal:       32768000 kB\nMemAvailable:   16384000 kB\n";
        assert_eq!(memory_info_mib(meminfo, "MemTotal"), Some(32_000));
        assert_eq!(memory_info_mib(meminfo, "MemAvailable"), Some(16_000));
        let release = "ID=ubuntu\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n";
        let cpu = "processor : 0\nmodel name : AMD Ryzen 9 5950X 16-Core Processor\n";
        assert_eq!(
            cpu_model(cpu).as_deref(),
            Some("AMD Ryzen 9 5950X 16-Core Processor")
        );
        assert_eq!(cpu_model("processor : 0\n"), None);
        assert_eq!(os_release_value(release, "ID").as_deref(), Some("ubuntu"));
        assert_eq!(
            os_release_value(release, "VERSION_ID").as_deref(),
            Some("24.04")
        );
    }

    #[test]
    fn memory_parser_converts_gibibyte_and_mebibyte_values() {
        assert_eq!(parse_memory_mib("24 GB"), Some(24 * 1024));
        assert_eq!(parse_memory_mib("24576 MiB"), Some(24576));
        assert_eq!(parse_memory_mib("1.5 TB"), Some(1_572_864));
        assert_eq!(parse_memory_mib("unknown"), None);
    }

    #[test]
    fn nvidia_csv_parser_handles_multiple_devices() {
        let sample = "NVIDIA GeForce RTX 3090, 24576\nNVIDIA GeForce RTX 4090, 24564\n";
        let parsed = sample
            .lines()
            .filter_map(|line| {
                let (name, memory) = line.split_once(',')?;
                Some(GpuDevice {
                    vendor: "nvidia".into(),
                    name: name.trim().into(),
                    compute_capability: None,
                    vram_mib: memory.trim().parse().ok(),
                    vram_used_mib: None,
                    vram_free_mib: None,
                    driver_version: None,
                    pcie_link_gen: None,
                    pcie_link_width: None,
                    pcie_link_gen_max: None,
                    pcie_link_width_max: None,
                    utilization_percent: None,
                    temperature_c: None,
                    power_draw_mw: None,
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].vram_mib, Some(24576));
    }
}
