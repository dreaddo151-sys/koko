//! Read-only host and local runtime discovery for portable DREAD installations.
//!
//! Probes only a small allowlist of known system utilities. It never invokes a
//! shell, installs software, changes drivers, or starts an inference process.

pub mod repair;

use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
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
    /// Stable provider identity where available (for example NVIDIA GPU UUID).
    pub device_id: Option<String>,
    /// Current enumeration index; useful for display, not a persistent identity.
    pub device_index: Option<u32>,
    /// PCI address such as 00000000:01:00.0 when exposed by the driver.
    pub pci_bus_id: Option<String>,
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
pub struct StoragePartition {
    pub name: String,
    pub filesystem: Option<String>,
    pub mount_points: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PhysicalStorageDevice {
    pub name: String,
    pub model: Option<String>,
    pub transport: Option<String>,
    pub size_mib: Option<u64>,
    pub partitions: Vec<StoragePartition>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CpuTopology {
    logical_threads: Option<usize>,
    sockets: Option<usize>,
    cores_per_socket: Option<usize>,
    threads_per_core: Option<usize>,
    numa_nodes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CudaToolkit {
    pub version: String,
    pub compiler_path: String,
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
    /// Logical processors visible to the OS, distinct from process CPU quota.
    pub cpu_logical_threads: Option<usize>,
    pub cpu_sockets: Option<usize>,
    pub cpu_cores_per_socket: Option<usize>,
    pub cpu_threads_per_core: Option<usize>,
    pub cpu_numa_nodes: Option<usize>,
    /// CUDA compatibility level advertised by the installed NVIDIA driver.
    pub cuda_version: Option<String>,
    /// Preferred CUDA Toolkit compiler version (PATH-first), when installed.
    pub cuda_toolkit_version: Option<String>,
    /// All distinct CUDA Toolkit compilers discovered on this host.
    pub cuda_toolkits: Vec<CudaToolkit>,
    pub cpu_threads: usize,
    pub memory_total_mib: Option<u64>,
    pub memory_available_mib: Option<u64>,
    pub root_disk_total_mib: Option<u64>,
    pub root_disk_available_mib: Option<u64>,
    pub storage_devices: Vec<StorageDevice>,
    /// Physical whole-disk inventory, including disks without mounted filesystems.
    pub physical_storage_devices: Vec<PhysicalStorageDevice>,
    pub gpu_devices: Vec<GpuDevice>,
    /// Optional executable path; this does not imply a llama-server process is running.
    pub llama_server: Option<String>,
    pub nvidia_smi_available: bool,
    pub rocm_smi_available: bool,
    pub xpu_smi_available: bool,
    /// Selected backend; actual server lifecycle is on-demand under the GPU Supervisor.
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

fn run_probe_with_timeout(program: &Path, args: &[&str], timeout_seconds: u64) -> Option<String> {
    let timeout_arg = format!("{timeout_seconds}s");
    let output = if let Some(timeout) = find_binary(&["timeout"]) {
        Command::new(timeout)
            .arg("--signal=KILL")
            .arg(&timeout_arg)
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

fn run_probe(program: &Path, args: &[&str]) -> Option<String> {
    // Keep ordinary host probes short so a stuck utility cannot stall profile requests.
    run_probe_with_timeout(program, args, 5)
}

fn optional_csv_field(fields: &[&str], index: usize) -> Option<String> {
    fields
        .get(index)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("N/A"))
        .map(ToString::to_string)
}

fn parse_nvidia_csv(output: &str) -> Vec<GpuDevice> {
    output
        .lines()
        .filter_map(|line| {
            let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
            // Preferred query format prefixes the stable UUID and PCI address.
            // Legacy format is accepted for older query fixtures and fallback tools.
            let has_identity = fields.len() >= 15
                && fields
                    .first()
                    .and_then(|value| value.parse::<u32>().ok())
                    .is_some()
                && fields.get(3).is_some_and(|name| !name.trim().is_empty());
            let (device_index, device_id, pci_bus_id, offset) = if has_identity {
                (
                    fields.first().and_then(|value| value.parse::<u32>().ok()),
                    optional_csv_field(&fields, 1),
                    optional_csv_field(&fields, 2),
                    3usize,
                )
            } else {
                (None, None, None, 0usize)
            };
            if fields.len() < offset + 12 {
                return None;
            }
            let name = fields[offset].trim();
            if name.is_empty() || name.eq_ignore_ascii_case("N/A") {
                return None;
            }
            let value = |index: usize| fields.get(offset + index).copied().unwrap_or("");
            Some(GpuDevice {
                vendor: "nvidia".into(),
                name: name.into(),
                device_id,
                device_index,
                pci_bus_id,
                compute_capability: optional_csv_field(
                    &fields,
                    if has_identity { offset + 12 } else { 12 },
                ),
                vram_mib: value(1).parse().ok(),
                vram_used_mib: value(2).parse().ok(),
                vram_free_mib: value(3).parse().ok(),
                driver_version: optional_csv_field(&fields, offset + 4),
                pcie_link_gen: value(5).parse().ok(),
                pcie_link_width: value(6).parse().ok(),
                pcie_link_gen_max: value(7).parse().ok(),
                pcie_link_width_max: value(8).parse().ok(),
                utilization_percent: value(9).parse().ok(),
                temperature_c: value(10).parse().ok(),
                power_draw_mw: value(11)
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
            "--query-gpu=index,uuid,pci.bus_id,name,memory.total,memory.used,memory.free,driver_version,pcie.link.gen.current,pcie.link.width.current,pcie.link.gen.max,pcie.link.width.max,utilization.gpu,temperature.gpu,power.draw,compute_cap",
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
            "--query-gpu=index,uuid,pci.bus_id,name,memory.total,memory.used,memory.free,driver_version,pcie.link.gen.current,pcie.link.width.current,pcie.link.gen.max,pcie.link.width.max,utilization.gpu,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ],
    ) {
        let devices = parse_nvidia_csv(&output);
        if !devices.is_empty() {
            return devices;
        }
    }

    if let Some(output) = run_probe(
        binary,
        &[
            "--query-gpu=index,uuid,pci.bus_id,name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    ) {
        let devices = output
            .lines()
            .filter_map(|line| {
                let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
                if fields.len() < 5 {
                    return None;
                }
                let name = fields[3];
                if name.is_empty() || name.eq_ignore_ascii_case("N/A") {
                    return None;
                }
                Some(GpuDevice {
                    vendor: "nvidia".into(),
                    name: name.into(),
                    device_id: optional_csv_field(&fields, 1),
                    device_index: fields[0].parse().ok(),
                    pci_bus_id: optional_csv_field(&fields, 2),
                    compute_capability: None,
                    vram_mib: fields[4].parse().ok(),
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
        if !devices.is_empty() {
            return devices;
        }
    }

    // Last-resort compatibility path for older nvidia-smi versions that do
    // not expose UUID or PCI query fields. Device identity remains unknown.
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
            if name.is_empty() || name.eq_ignore_ascii_case("N/A") {
                return None;
            }
            Some(GpuDevice {
                vendor: "nvidia".into(),
                name: name.into(),
                device_id: None,
                device_index: None,
                pci_bus_id: None,
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

fn intel_string<'a>(object: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .filter(|value| {
            let normalized = value.trim();
            !normalized.is_empty()
                && !normalized.eq_ignore_ascii_case("n/a")
                && !normalized.eq_ignore_ascii_case("unknown")
                && !normalized.eq_ignore_ascii_case("not supported")
        })
}

fn intel_number(object: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = object.get(*key)?;
        if let Some(number) = value.as_f64() {
            return number.is_finite().then_some(number);
        }
        let text = value.as_str()?.trim();
        if text.is_empty()
            || text.eq_ignore_ascii_case("n/a")
            || text.eq_ignore_ascii_case("unknown")
            || text.eq_ignore_ascii_case("not supported")
        {
            return None;
        }
        let numeric = text
            .split_whitespace()
            .next()
            .unwrap_or(text)
            .trim_end_matches('%');
        numeric
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
    })
}

fn intel_nested_number(object: &Value, path: &[&str]) -> Option<f64> {
    let value = path.iter().try_fold(object, |value, key| value.get(*key))?;
    if let Some(number) = value.as_f64() {
        return number.is_finite().then_some(number);
    }
    let text = value.as_str()?.trim();
    if text.is_empty()
        || text.eq_ignore_ascii_case("n/a")
        || text.eq_ignore_ascii_case("unknown")
        || text.eq_ignore_ascii_case("not supported")
    {
        return None;
    }
    text.split_whitespace()
        .next()
        .unwrap_or(text)
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

fn intel_discovery_memory_mib(object: &Value) -> Option<f64> {
    if let Some(value) = intel_number(object, &["memory_physical_size_mib", "memory_total_mib"]) {
        return Some(value);
    }
    for key in ["Memory Physical Size", "memory_physical_size"] {
        let Some(text) = object.get(key).and_then(Value::as_str) else {
            continue;
        };
        let mut parts = text.split_whitespace();
        let amount = parts.next()?.parse::<f64>().ok()?;
        if !amount.is_finite() || amount < 0.0 {
            return None;
        }
        let unit = parts.next().unwrap_or("");
        return match unit.to_ascii_lowercase().as_str() {
            "mib" => Some(amount),
            "gib" => Some(amount * 1024.0),
            "mb" => Some(amount * 1_000_000.0 / 1_048_576.0),
            "gb" => Some(amount * 1_000_000_000.0 / 1_048_576.0),
            _ => None,
        };
    }
    None
}

fn intel_u32(object: &Value, keys: &[&str]) -> Option<u32> {
    intel_number(object, keys)
        .filter(|value| *value >= 0.0 && *value <= f64::from(u32::MAX))
        .map(|value| value as u32)
}

/// Parse documented Intel XPU Manager query JSON output.
fn parse_intel_query_json(output: &str) -> Vec<GpuDevice> {
    let Ok(report) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    let Some(gpus) = report.get("gpus").and_then(Value::as_array) else {
        return Vec::new();
    };
    gpus.iter()
        .filter_map(|gpu| {
            if !gpu.is_object() {
                return None;
            }
            let name = intel_string(gpu, &["product_name", "name", "device_name"])?;
            let pci = intel_string(gpu, &["pci_bus_id"])
                .or_else(|| gpu.pointer("/pci/bus_id").and_then(Value::as_str));
            let uuid = intel_string(gpu, &["gpu_uuid", "uuid", "soc_uuid"]);
            let index = intel_u32(gpu, &["device_index", "index"]).or_else(|| {
                intel_string(gpu, &["device_id"]).and_then(|value| value.parse::<u32>().ok())
            });

            let total = intel_nested_number(gpu, &["fb_memory_usage", "total"])
                .or_else(|| intel_number(gpu, &["memory_total_mib"]));
            let used = intel_nested_number(gpu, &["fb_memory_usage", "used"])
                .or_else(|| intel_number(gpu, &["memory_used_mib"]));
            let free = intel_nested_number(gpu, &["fb_memory_usage", "free"])
                .or_else(|| intel_number(gpu, &["memory_free_mib"]));

            let pcie_gen =
                intel_nested_number(gpu, &["pci", "gpu_link_info", "pcie_generation", "current"])
                    .or_else(|| intel_number(gpu, &["pcie_generation_current"]));
            let pcie_width =
                intel_nested_number(gpu, &["pci", "gpu_link_info", "link_width", "current"])
                    .or_else(|| intel_number(gpu, &["pcie_link_width_current"]));

            let power_w = intel_nested_number(gpu, &["gpu_power_readings", "average_power_draw"])
                .or_else(|| intel_number(gpu, &["power_draw_w"]));

            Some(GpuDevice {
                vendor: "intel".into(),
                name: name.trim().to_string(),
                device_id: uuid.map(str::to_string),
                device_index: index,
                pci_bus_id: pci.map(str::to_string),
                compute_capability: None,
                vram_mib: total.and_then(|value| {
                    (value >= 0.0 && value <= u64::MAX as f64).then_some(value.round() as u64)
                }),
                vram_used_mib: used.and_then(|value| {
                    (value >= 0.0 && value <= u64::MAX as f64).then_some(value.round() as u64)
                }),
                vram_free_mib: free.and_then(|value| {
                    (value >= 0.0 && value <= u64::MAX as f64).then_some(value.round() as u64)
                }),
                driver_version: intel_string(&report, &["umd_version"]).map(str::to_string),
                pcie_link_gen: pcie_gen.and_then(|value| {
                    (value >= 0.0 && value <= f64::from(u32::MAX)).then_some(value as u32)
                }),
                pcie_link_width: pcie_width.and_then(|value| {
                    (value >= 0.0 && value <= f64::from(u32::MAX)).then_some(value as u32)
                }),
                pcie_link_gen_max: intel_nested_number(
                    gpu,
                    &["pci", "gpu_link_info", "pcie_generation", "max"],
                )
                .and_then(|value| {
                    (value >= 0.0 && value <= f64::from(u32::MAX)).then_some(value as u32)
                }),
                pcie_link_width_max: intel_nested_number(
                    gpu,
                    &["pci", "gpu_link_info", "link_width", "max"],
                )
                .and_then(|value| {
                    (value >= 0.0 && value <= f64::from(u32::MAX)).then_some(value as u32)
                }),
                utilization_percent: intel_nested_number(gpu, &["utilization", "gpu"]).and_then(
                    |value| {
                        (0.0..=100.0)
                            .contains(&value)
                            .then_some(value.round() as u32)
                    },
                ),
                temperature_c: intel_nested_number(gpu, &["temperature", "gpu_current_temp"])
                    .and_then(|value| {
                        (value >= 0.0 && value <= f64::from(u32::MAX))
                            .then_some(value.round() as u32)
                    }),
                power_draw_mw: power_w.and_then(|value| {
                    (value >= 0.0 && value <= (u64::MAX as f64 / 1000.0))
                        .then_some((value * 1000.0).round() as u64)
                }),
            })
        })
        .collect()
}

/// Fallback for Intel XPU Manager versions that expose discovery JSON
/// but do not support the richer query report.
fn parse_intel_discovery_json(output: &str) -> Vec<GpuDevice> {
    let Ok(root) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    let items = root
        .get("devices")
        .or_else(|| root.get("gpus"))
        .and_then(Value::as_array)
        .or_else(|| root.as_array());
    let Some(items) = items else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|device| {
            if !device.is_object() {
                return None;
            }
            let name = intel_string(
                device,
                &["device_name", "Device Name", "product_name", "name"],
            )?;
            let pci = intel_string(
                device,
                &["pci_bdf_address", "PCI BDF Address", "pci_bus_id"],
            );
            let uuid = intel_string(device, &["soc_uuid", "SOC UUID", "gpu_uuid", "uuid"]);
            let index = intel_u32(device, &["device_id", "Device ID", "device_index", "index"]);
            let memory = intel_discovery_memory_mib(device);
            let driver = intel_string(device, &["umd_version", "UMD Version"]);
            Some(GpuDevice {
                vendor: "intel".into(),
                name: name.trim().to_string(),
                device_id: uuid.map(str::to_string),
                device_index: index,
                pci_bus_id: pci.map(str::to_string),
                compute_capability: None,
                vram_mib: memory.and_then(|value| {
                    (value >= 0.0 && value <= u64::MAX as f64).then_some(value.round() as u64)
                }),
                vram_used_mib: None,
                vram_free_mib: None,
                driver_version: driver.map(str::to_string),
                pcie_link_gen: None,
                pcie_link_width: None,
                pcie_link_gen_max: intel_u32(device, &["pcie_generation", "PCIe Generation"]),
                pcie_link_width_max: intel_u32(
                    device,
                    &["pcie_max_link_width", "PCIe Max Link Width"],
                ),
                utilization_percent: None,
                temperature_c: None,
                power_draw_mw: None,
            })
        })
        .collect()
}

/// Parse the documented xpu-smi --query-gpu CSV identity fallback.
fn parse_intel_identity_csv(output: &str) -> Vec<GpuDevice> {
    output
        .lines()
        .filter_map(|line| {
            let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
            if fields.len() < 4 {
                return None;
            }
            let device_index = fields.first()?.parse::<u32>().ok()?;
            let name = fields.get(1)?.trim();
            if name.is_empty()
                || name.eq_ignore_ascii_case("n/a")
                || name.eq_ignore_ascii_case("unknown")
            {
                return None;
            }
            let optional = |index: usize| {
                fields
                    .get(index)
                    .map(|value| value.trim())
                    .filter(|value| {
                        !value.is_empty()
                            && !value.eq_ignore_ascii_case("n/a")
                            && !value.eq_ignore_ascii_case("unknown")
                    })
                    .map(ToString::to_string)
            };
            Some(GpuDevice {
                vendor: "intel".into(),
                name: name.into(),
                device_id: optional(2),
                device_index: Some(device_index),
                pci_bus_id: optional(3),
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

fn intel_devices(binary: &Path) -> Vec<GpuDevice> {
    // XPU Manager's first driver query may initialize Level Zero and collect a
    // sample. Bound it independently from the short host-inventory probes.
    if let Some(output) = run_probe_with_timeout(binary, &["--timeout=8", "query", "--json"], 12) {
        let devices = parse_intel_query_json(&output);
        if !devices.is_empty() {
            return devices;
        }
    }
    if let Some(output) =
        run_probe_with_timeout(binary, &["--timeout=6", "discovery", "--json"], 10)
    {
        let devices = parse_intel_discovery_json(&output);
        if !devices.is_empty() {
            return devices;
        }
    }
    if let Some(output) = run_probe_with_timeout(
        binary,
        &[
            "--timeout=6",
            "--query-gpu=index,name,uuid,pci.bus_id",
            "--format=csv,noheader,nounits",
        ],
        10,
    ) {
        let devices = parse_intel_identity_csv(&output);
        if !devices.is_empty() {
            return devices;
        }
    }
    Vec::new()
}

fn amd_gpu_rows(output: &str) -> Vec<Value> {
    let Ok(root) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    root.get("gpu_data")
        .or_else(|| root.get("gpus"))
        .and_then(Value::as_array)
        .or_else(|| root.as_array())
        .cloned()
        .unwrap_or_default()
}

fn amd_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn amd_string(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(value) => value.trim().to_string(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    if text.is_empty()
        || text.eq_ignore_ascii_case("n/a")
        || text.eq_ignore_ascii_case("unknown")
        || text.eq_ignore_ascii_case("not supported")
    {
        None
    } else {
        Some(text)
    }
}

fn amd_string_at(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| amd_path(value, path).and_then(amd_string))
}

fn amd_number_and_unit(value: &Value) -> Option<(f64, Option<String>)> {
    let (raw, unit) = if let Some(object) = value.as_object() {
        (
            object.get("value")?,
            object.get("unit").and_then(amd_string),
        )
    } else {
        (value, None)
    };

    let number = match raw {
        Value::Number(value) => value.as_f64()?,
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty()
                || value.eq_ignore_ascii_case("n/a")
                || value.eq_ignore_ascii_case("unknown")
                || value.eq_ignore_ascii_case("not supported")
            {
                return None;
            }
            if let Ok(number) = value.parse::<f64>() {
                number
            } else {
                let (number, suffix) = value.split_once(char::is_whitespace)?;
                let number = number.trim().parse::<f64>().ok()?;
                let suffix = suffix.trim();
                if suffix.is_empty() {
                    return None;
                }
                return number
                    .is_finite()
                    .then_some((number, Some(suffix.to_string())));
            }
        }
        _ => return None,
    };
    number.is_finite().then_some((number, unit))
}

fn amd_number_at(value: &Value, paths: &[&[&str]]) -> Option<(f64, Option<String>)> {
    paths
        .iter()
        .find_map(|path| amd_path(value, path).and_then(amd_number_and_unit))
}

fn amd_memory_mib(value: &(f64, Option<String>)) -> Option<u64> {
    let (number, unit) = value;
    if *number < 0.0 {
        return None;
    }
    let unit = unit.as_deref().unwrap_or("MB").to_ascii_lowercase();
    let mib = match unit.as_str() {
        "b" | "byte" | "bytes" => number / (1024.0 * 1024.0),
        "kb" | "kib" => number / 1024.0,
        "gb" | "gib" => number * 1024.0,
        "tb" | "tib" => number * 1024.0 * 1024.0,
        "mb" | "mib" | "" => *number,
        _ => return None,
    };
    (mib.is_finite() && mib >= 0.0 && mib <= u64::MAX as f64).then_some(mib.round() as u64)
}

fn amd_power_mw(value: &(f64, Option<String>)) -> Option<u64> {
    let (number, unit) = value;
    if *number < 0.0 {
        return None;
    }
    let watts = match unit.as_deref().unwrap_or("W").to_ascii_lowercase().as_str() {
        "w" | "watt" | "watts" => *number,
        "mw" | "milliwatt" | "milliwatts" => *number / 1000.0,
        _ => return None,
    };
    (watts.is_finite() && watts <= u64::MAX as f64 / 1000.0)
        .then_some((watts * 1000.0).round() as u64)
}

fn amd_percent(value: &(f64, Option<String>)) -> Option<u32> {
    let (number, unit) = value;
    if unit
        .as_deref()
        .is_some_and(|unit| unit != "%" && !unit.eq_ignore_ascii_case("percent"))
    {
        return None;
    }
    (0.0..=100.0)
        .contains(number)
        .then_some(number.round() as u32)
}

fn amd_pcie_generation(value: &Value) -> Option<u32> {
    if let Some(text) = amd_string(value) {
        let lower = text.to_ascii_lowercase();
        if let Some(generation) = lower
            .strip_prefix("gen")
            .and_then(|suffix| suffix.trim().parse::<u32>().ok())
        {
            return Some(generation);
        }
    }
    let (speed, _) = amd_number_and_unit(value)?;
    [
        (2.5, 1),
        (5.0, 2),
        (8.0, 3),
        (16.0, 4),
        (32.0, 5),
        (64.0, 6),
    ]
    .iter()
    .find_map(|(rate, generation)| ((speed - rate).abs() < 0.2).then_some(*generation))
}

fn parse_amd_smi_json(list_json: &str, static_json: &str, metric_json: &str) -> Vec<GpuDevice> {
    let list_rows = amd_gpu_rows(list_json);
    let static_rows = amd_gpu_rows(static_json);
    let metric_rows = amd_gpu_rows(metric_json);

    let mut records = BTreeMap::<u32, (Option<Value>, Option<Value>, Option<Value>)>::new();
    for row in list_rows {
        let Some(index) = amd_number_at(&row, &[&["gpu"], &["gpu_index"], &["id"]])
            .map(|(number, _)| number)
            .filter(|number| *number >= 0.0 && *number <= f64::from(u32::MAX))
            .map(|number| number as u32)
        else {
            continue;
        };
        records.entry(index).or_default().0 = Some(row);
    }
    for row in static_rows {
        let Some(index) = amd_number_at(&row, &[&["gpu"], &["gpu_index"], &["id"]])
            .map(|(number, _)| number)
            .filter(|number| *number >= 0.0 && *number <= f64::from(u32::MAX))
            .map(|number| number as u32)
        else {
            continue;
        };
        records.entry(index).or_default().1 = Some(row);
    }
    for row in metric_rows {
        let Some(index) = amd_number_at(&row, &[&["gpu"], &["gpu_index"], &["id"]])
            .map(|(number, _)| number)
            .filter(|number| *number >= 0.0 && *number <= f64::from(u32::MAX))
            .map(|number| number as u32)
        else {
            continue;
        };
        records.entry(index).or_default().2 = Some(row);
    }

    records
        .into_iter()
        .filter_map(|(index, (list, static_info, metrics))| {
            let list = list.unwrap_or(Value::Null);
            let static_info = static_info.unwrap_or(Value::Null);
            let metrics = metrics.unwrap_or(Value::Null);

            let name = amd_string_at(
                &static_info,
                &[
                    &["asic", "market_name"],
                    &["board", "product_name"],
                    &["asic", "product_name"],
                    &["product_name"],
                    &["name"],
                ],
            )
            .or_else(|| amd_string_at(&list, &[&["market_name"], &["product_name"], &["name"]]))?;

            let uuid =
                amd_string_at(&list, &[&["uuid"], &["gpu_uuid"], &["hip_uuid"]]).or_else(|| {
                    amd_string_at(
                        &static_info,
                        &[&["asic", "asic_serial"], &["asic", "serial"]],
                    )
                });
            let pci_bus_id = amd_string_at(&list, &[&["bdf"], &["pci_bus_id"], &["bus", "bdf"]])
                .or_else(|| amd_string_at(&static_info, &[&["bus", "bdf"], &["bdf"]]));

            let total_vram = amd_number_at(
                &static_info,
                &[&["vram", "size"], &["vram", "total"], &["vram_size"]],
            )
            .and_then(|value| amd_memory_mib(&value))
            .or_else(|| {
                amd_number_at(
                    &metrics,
                    &[&["mem_usage", "total_vram"], &["memory", "total_vram"]],
                )
                .and_then(|value| amd_memory_mib(&value))
            });
            let used_vram = amd_number_at(
                &metrics,
                &[
                    &["mem_usage", "used_vram"],
                    &["mem_usage", "vram_used"],
                    &["memory", "used_vram"],
                ],
            )
            .and_then(|value| amd_memory_mib(&value));
            let free_vram = amd_number_at(
                &metrics,
                &[
                    &["mem_usage", "free_vram"],
                    &["mem_usage", "vram_free"],
                    &["memory", "free_vram"],
                ],
            )
            .and_then(|value| amd_memory_mib(&value))
            .or_else(|| {
                total_vram
                    .zip(used_vram)
                    .map(|(total, used)| total.saturating_sub(used))
            });

            let current_pcie_width = amd_number_at(&metrics, &[&["pcie", "width"]])
                .map(|(number, _)| number)
                .filter(|number| *number >= 0.0 && *number <= f64::from(u32::MAX))
                .map(|number| number as u32);
            let current_pcie_gen =
                amd_path(&metrics, &["pcie", "speed"]).and_then(amd_pcie_generation);
            let maximum_pcie_width = amd_number_at(
                &static_info,
                &[&["bus", "max_pcie_width"], &["bus", "max_link_width"]],
            )
            .map(|(number, _)| number)
            .filter(|number| *number >= 0.0 && *number <= f64::from(u32::MAX))
            .map(|number| number as u32);
            let maximum_pcie_gen =
                amd_string_at(&static_info, &[&["bus", "pcie_interface_version"]])
                    .and_then(|value| {
                        value
                            .to_ascii_lowercase()
                            .split_whitespace()
                            .last()
                            .and_then(|value| value.parse::<u32>().ok())
                    })
                    .or_else(|| {
                        amd_string_at(&static_info, &[&["bus", "max_pcie_speed"]])
                            .and_then(|value| amd_pcie_generation(&Value::String(value)))
                    });

            let driver_version = amd_string_at(
                &static_info,
                &[&["driver", "version"], &["driver", "amdgpu_version"]],
            );
            let utilization_percent = amd_number_at(
                &metrics,
                &[&["usage", "gfx_activity"], &["usage", "gfx_utilization"]],
            )
            .and_then(|value| amd_percent(&value));
            let temperature_c = amd_number_at(
                &metrics,
                &[
                    &["temperature", "hotspot"],
                    &["temperature", "edge"],
                    &["temperature", "gpu_hotspot"],
                ],
            )
            .map(|(number, _)| number)
            .filter(|number| *number >= 0.0 && *number <= f64::from(u32::MAX))
            .map(|number| number.round() as u32);
            let power_draw_mw = amd_number_at(
                &metrics,
                &[&["power", "socket_power"], &["power", "average_power"]],
            )
            .and_then(|value| amd_power_mw(&value));

            Some(GpuDevice {
                vendor: "amd".into(),
                name,
                device_id: uuid,
                device_index: Some(index),
                pci_bus_id,
                compute_capability: None,
                vram_mib: total_vram,
                vram_used_mib: used_vram,
                vram_free_mib: free_vram,
                driver_version,
                pcie_link_gen: current_pcie_gen,
                pcie_link_width: current_pcie_width,
                pcie_link_gen_max: maximum_pcie_gen,
                pcie_link_width_max: maximum_pcie_width,
                utilization_percent,
                temperature_c,
                power_draw_mw,
            })
        })
        .collect()
}

fn amd_smi_devices(binary: &Path) -> Vec<GpuDevice> {
    let list = run_probe_with_timeout(binary, &["list", "--json"], 5).unwrap_or_default();
    let static_info = run_probe_with_timeout(binary, &["static", "--json"], 5).unwrap_or_default();
    let metrics = run_probe_with_timeout(binary, &["metric", "--json"], 5).unwrap_or_default();
    parse_amd_smi_json(&list, &static_info, &metrics)
}

fn amd_rocm_smi_devices(binary: &Path) -> Vec<GpuDevice> {
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
            let device_index = line
                .split_once("GPU[")
                .and_then(|(_, rest)| rest.split_once(']'))
                .and_then(|(index, _)| index.trim().parse::<u32>().ok());
            Some(GpuDevice {
                vendor: "amd".into(),
                name: name.into(),
                device_id: None,
                device_index,
                pci_bus_id: None,
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
                let device_index = u32::try_from(devices.len()).ok();
                devices.push(GpuDevice {
                    vendor: "apple".into(),
                    name,
                    device_id: None,
                    device_index,
                    pci_bus_id: None,
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
        let device_index = u32::try_from(devices.len()).ok();
        devices.push(GpuDevice {
            vendor: "apple".into(),
            name,
            device_id: None,
            device_index,
            pci_bus_id: None,
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

fn cpu_logical_threads(contents: &str) -> Option<usize> {
    let count = contents
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "processor")
        })
        .count();
    (count > 0).then_some(count)
}

fn parse_lscpu_topology(output: &str) -> CpuTopology {
    let Ok(payload) = serde_json::from_str::<Value>(output) else {
        return CpuTopology::default();
    };
    let Some(rows) = payload.get("lscpu").and_then(Value::as_array) else {
        return CpuTopology::default();
    };
    let value = |name: &str| {
        rows.iter().find_map(|row| {
            let field = row
                .get("field")
                .and_then(Value::as_str)?
                .trim()
                .trim_end_matches(':')
                .trim();
            if field != name {
                return None;
            }
            row.get("data")
                .and_then(Value::as_str)?
                .split_whitespace()
                .next()?
                .parse::<usize>()
                .ok()
        })
    };
    CpuTopology {
        logical_threads: value("CPU(s)"),
        sockets: value("Socket(s)"),
        cores_per_socket: value("Core(s) per socket"),
        threads_per_core: value("Thread(s) per core"),
        numa_nodes: value("NUMA node(s)"),
    }
}

fn discover_cpu_topology() -> CpuTopology {
    find_binary(&["lscpu"])
        .and_then(|binary| run_probe(&binary, &["--json"]))
        .map(|output| parse_lscpu_topology(&output))
        .unwrap_or_default()
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
        let (_, version) = line
            .split_once("CUDA UMD Version:")
            .or_else(|| line.split_once("CUDA Version:"))?;
        let version = version.split_whitespace().next()?;
        (!version.is_empty() && version != "N/A").then(|| version.to_string())
    })
}

fn nvidia_cuda_version(binary: &Path) -> Option<String> {
    parse_nvidia_cuda_version(&run_probe(binary, &[])?)
}

fn find_cuda_nvcc_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(binary) = find_binary(&["nvcc"]) {
        candidates.push(binary);
    }
    candidates.push(PathBuf::from("/usr/local/cuda/bin/nvcc"));

    let mut versioned = Vec::new();
    if let Ok(entries) = fs::read_dir("/usr/local") {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("cuda-") {
                versioned.push(entry.path().join("bin/nvcc"));
            }
        }
    }
    versioned.sort();
    candidates.extend(versioned);

    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|path| path.is_file())
        .filter(|path| {
            let identity = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            seen.insert(identity)
        })
        .collect()
}

fn parse_nvcc_version(output: &str) -> Option<String> {
    let (_, release) = output.split_once("release ")?;
    let version = release
        .split(|character: char| character == ',' || character.is_whitespace())
        .next()?;
    (!version.is_empty()).then(|| version.to_string())
}

fn parse_cuda_toolkits(
    candidates: impl IntoIterator<Item = (PathBuf, String)>,
) -> Vec<CudaToolkit> {
    candidates
        .into_iter()
        .filter_map(|(compiler_path, output)| {
            Some(CudaToolkit {
                version: parse_nvcc_version(&output)?,
                compiler_path: compiler_path.display().to_string(),
            })
        })
        .collect()
}

fn cuda_toolkits() -> Vec<CudaToolkit> {
    let candidates = find_cuda_nvcc_candidates()
        .into_iter()
        .filter_map(|compiler| Some((compiler.clone(), run_probe(&compiler, &["--version"])?)))
        .collect::<Vec<_>>();
    parse_cuda_toolkits(candidates)
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
        &["-B1", "--output=source,fstype,size,used,avail,target"],
    ) else {
        return Vec::new();
    };
    parse_df_storage(&output)
}

fn parse_lsblk_storage(output: &str) -> Vec<PhysicalStorageDevice> {
    fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
        value.get(key)?.as_str().filter(|text| !text.is_empty())
    }

    fn mount_points(value: &Value) -> Vec<String> {
        value
            .get("mountpoints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .collect()
    }

    let Ok(document) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    document
        .get("blockdevices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|device| text_field(device, "type") == Some("disk"))
        .map(|device| {
            let partitions = device
                .get("children")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|partition| {
                    let name = text_field(partition, "name")?.to_string();
                    Some(StoragePartition {
                        name,
                        filesystem: text_field(partition, "fstype").map(str::to_string),
                        mount_points: mount_points(partition),
                    })
                })
                .collect();
            PhysicalStorageDevice {
                name: text_field(device, "name").unwrap_or_default().to_string(),
                model: text_field(device, "model").map(str::to_string),
                transport: text_field(device, "tran").map(str::to_string),
                size_mib: device
                    .get("size")
                    .and_then(|size| size.as_u64().or_else(|| size.as_str()?.parse().ok()))
                    .map(|bytes| bytes / 1024 / 1024),
                partitions,
            }
        })
        .collect()
}

fn physical_storage_devices() -> Vec<PhysicalStorageDevice> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let Some(binary) = find_binary(&["lsblk"]) else {
        return Vec::new();
    };
    let Some(output) = run_probe(
        &binary,
        &[
            "--json",
            "--bytes",
            "--output",
            "NAME,TYPE,SIZE,FSTYPE,MOUNTPOINTS,MODEL,TRAN",
        ],
    ) else {
        return Vec::new();
    };
    parse_lsblk_storage(&output)
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
    let amd_smi = find_binary(if cfg!(windows) {
        &["amd-smi.exe", "amd-smi"]
    } else {
        &["amd-smi"]
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
    let amd_inventory = amd_smi
        .as_ref()
        .map(|binary| amd_smi_devices(binary))
        .unwrap_or_default();
    if amd_inventory.is_empty() {
        if let Some(binary) = &amd {
            gpu_devices.extend(amd_rocm_smi_devices(binary));
        }
    } else {
        gpu_devices.extend(amd_inventory);
    }
    if let Some(binary) = &intel {
        gpu_devices.extend(intel_devices(binary));
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
    let topology = discover_cpu_topology();
    let cpu_physical_cores = cpu_physical_cores(&cpuinfo).or_else(|| {
        topology
            .cores_per_socket
            .zip(topology.sockets)
            .map(|(cores, sockets)| cores.saturating_mul(sockets))
    });
    let cpu_logical_threads = topology
        .logical_threads
        .or_else(|| cpu_logical_threads(&cpuinfo));
    let cuda_version = nvidia.as_deref().and_then(nvidia_cuda_version);
    let cuda_toolkits = cuda_toolkits();
    let cuda_toolkit_version = cuda_toolkits.first().map(|toolkit| toolkit.version.clone());
    let physical_storage_devices = physical_storage_devices();

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
        cpu_logical_threads,
        cpu_sockets: topology.sockets,
        cpu_cores_per_socket: topology.cores_per_socket,
        cpu_threads_per_core: topology.threads_per_core,
        cpu_numa_nodes: topology.numa_nodes,
        cuda_version,
        cuda_toolkit_version,
        cuda_toolkits,
        cpu_threads: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        memory_total_mib: memory_info_mib(&meminfo, "MemTotal"),
        memory_available_mib: memory_info_mib(&meminfo, "MemAvailable"),
        root_disk_total_mib,
        root_disk_available_mib,
        storage_devices,
        physical_storage_devices,
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
    use serde_json::json;

    #[test]
    fn cpu_topology_counts_unique_physical_and_core_pairs() {
        let sample = "processor : 0\nphysical id : 0\ncore id : 0\n\nprocessor : 1\nphysical id : 0\ncore id : 0\n\nprocessor : 2\nphysical id : 0\ncore id : 1\n\nprocessor : 3\nphysical id : 1\ncore id : 0\n";
        assert_eq!(cpu_physical_cores(sample), Some(3));
        assert_eq!(cpu_physical_cores("processor : 0\nprocessor : 1\n"), None);
    }

    #[test]
    fn lscpu_json_parser_reports_sockets_cores_threads_and_numa() {
        let output = r#"{
            "lscpu": [
                {"field": "CPU(s):", "data": "12"},
                {"field": "Thread(s) per core:", "data": "2"},
                {"field": "Core(s) per socket:", "data": "6"},
                {"field": "Socket(s):", "data": "1"},
                {"field": "NUMA node(s):", "data": "1"}
            ]
        }"#;
        let topology = parse_lscpu_topology(output);
        assert_eq!(topology.logical_threads, Some(12));
        assert_eq!(topology.threads_per_core, Some(2));
        assert_eq!(topology.cores_per_socket, Some(6));
        assert_eq!(topology.sockets, Some(1));
        assert_eq!(topology.numa_nodes, Some(1));
        assert_eq!(parse_lscpu_topology("not-json"), CpuTopology::default());
    }

    #[test]
    fn proc_cpuinfo_logical_processor_count_is_separate_from_quota() {
        let sample = "processor : 0\nmodel name : CPU\n\nprocessor : 1\nmodel name : CPU\n";
        assert_eq!(cpu_logical_threads(sample), Some(2));
        assert_eq!(cpu_logical_threads("model name : CPU\n"), None);
    }

    #[test]
    fn nvidia_cuda_banner_is_parsed_without_guessing() {
        let banner = "NVIDIA-SMI 550.90.07 Driver Version: 550.90.07 CUDA Version: 12.4\n";
        assert_eq!(parse_nvidia_cuda_version(banner).as_deref(), Some("12.4"));
        assert_eq!(parse_nvidia_cuda_version("CUDA Version: N/A"), None);
    }

    #[test]
    fn nvidia_cuda_umd_banner_is_parsed_as_driver_cuda_compatibility() {
        let banner = "NVIDIA-SMI 615.78.08 KMD Version: 615.78.08 CUDA UMD Version: 13.4\n";
        assert_eq!(parse_nvidia_cuda_version(banner).as_deref(), Some("13.4"));
    }

    #[test]
    fn nvcc_release_is_parsed_independently_from_driver_cuda_version() {
        let output = "Cuda compilation tools, release 13.0, V13.0.48\n";
        assert_eq!(parse_nvcc_version(output).as_deref(), Some("13.0"));
        assert_eq!(parse_nvcc_version("no compiler version"), None);
    }

    #[test]
    fn lsblk_parser_includes_unmounted_physical_disks_and_partition_mounts() {
        let sample = r#"{
          "blockdevices": [
            {"name":"loop0","type":"loop","size":4096},
            {"name":"nvme1n1","type":"disk","size":1024209543168,"model":"Sabrent","tran":"nvme",
             "children":[{"name":"nvme1n1p1","type":"part","size":64193879040,"fstype":"ext4","mountpoints":[null]}]},
            {"name":"nvme0n1","type":"disk","size":1000204886016,"model":"Samsung SSD 970 EVO 1TB","tran":"nvme",
             "children":[{"name":"nvme0n1p2","type":"part","size":999074824192,"fstype":"ext4","mountpoints":["/"]}]}
          ]
        }"#;
        let disks = parse_lsblk_storage(sample);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[0].name, "nvme1n1");
        assert_eq!(disks[0].model.as_deref(), Some("Sabrent"));
        assert_eq!(disks[0].transport.as_deref(), Some("nvme"));
        assert!(disks[0].partitions[0].mount_points.is_empty());
        assert_eq!(disks[1].partitions[0].mount_points, vec!["/"]);
        assert!(parse_lsblk_storage("not json").is_empty());
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
    fn mounted_storage_probe_uses_supported_gnu_df_output_format() {
        let mounts = mounted_storage_devices();
        assert!(
            mounts.iter().any(|mount| mount.mount_point == "/"),
            "the live GNU df probe should report the root filesystem"
        );
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
    fn cuda_toolkit_inventory_preserves_multiple_compilers_and_versions() {
        let inventory = parse_cuda_toolkits(vec![
            (
                PathBuf::from("/usr/local/cuda-12.8/bin/nvcc"),
                "Cuda compilation tools, release 12.8, V12.8.93".to_string(),
            ),
            (
                PathBuf::from("/usr/local/cuda-13.0/bin/nvcc"),
                "Cuda compilation tools, release 13.0, V13.0.48".to_string(),
            ),
        ]);
        assert_eq!(inventory.len(), 2);
        assert_eq!(inventory[0].version, "12.8");
        assert_eq!(inventory[0].compiler_path, "/usr/local/cuda-12.8/bin/nvcc");
        assert_eq!(inventory[1].version, "13.0");
        assert_eq!(inventory[1].compiler_path, "/usr/local/cuda-13.0/bin/nvcc");
    }

    #[test]
    fn intel_query_json_discovers_multiple_devices_with_documented_identity_and_metrics() {
        let sample = r#"{
            "umd_version": "1.17.39980",
            "attached_gpus": 2,
            "gpus": [
                {
                    "pci_bus_id": "0000:03:00.0",
                    "product_name": "Intel(R) Arc(TM) B580 Graphics",
                    "gpu_uuid": "00000000-0000-0003-0000-0000e20b8086",
                    "device_index": "0",
                    "fb_memory_usage": {"total": "12216", "used": "218.62", "free": "11997.38"},
                    "utilization": {"gpu": "27.50"},
                    "temperature": {"gpu_current_temp": "52.00"},
                    "gpu_power_readings": {"average_power_draw": "59.34"},
                    "pci": {
                        "gpu_link_info": {
                            "pcie_generation": {"current": "4", "max": "5"},
                            "link_width": {"current": "8", "max": "16"}
                        }
                    }
                },
                {
                    "pci_bus_id": "0000:4d:00.0",
                    "product_name": "Intel(R) Data Center GPU Max",
                    "gpu_uuid": "00000000-0000-0004-0000-0000e20b8086",
                    "device_index": "1",
                    "fb_memory_usage": {"total": "65536", "used": "4096", "free": "61440"},
                    "utilization": {"gpu": null},
                    "temperature": {"gpu_current_temp": null},
                    "gpu_power_readings": {"average_power_draw": null}
                }
            ]
        }"#;
        let parsed = parse_intel_query_json(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].vendor, "intel");
        assert_eq!(parsed[0].name, "Intel(R) Arc(TM) B580 Graphics");
        assert_eq!(
            parsed[0].device_id.as_deref(),
            Some("00000000-0000-0003-0000-0000e20b8086")
        );
        assert_eq!(parsed[0].device_index, Some(0));
        assert_eq!(parsed[0].pci_bus_id.as_deref(), Some("0000:03:00.0"));
        assert_eq!(parsed[0].vram_mib, Some(12216));
        assert_eq!(parsed[0].vram_used_mib, Some(219));
        assert_eq!(parsed[0].vram_free_mib, Some(11997));
        assert_eq!(parsed[0].driver_version.as_deref(), Some("1.17.39980"));
        assert_eq!(parsed[0].pcie_link_gen, Some(4));
        assert_eq!(parsed[0].pcie_link_width, Some(8));
        assert_eq!(parsed[0].pcie_link_gen_max, Some(5));
        assert_eq!(parsed[0].pcie_link_width_max, Some(16));
        assert_eq!(parsed[0].utilization_percent, Some(28));
        assert_eq!(parsed[0].temperature_c, Some(52));
        assert_eq!(parsed[0].power_draw_mw, Some(59_340));
        assert_eq!(parsed[1].device_index, Some(1));
        assert_eq!(parsed[1].utilization_percent, None);
        assert_eq!(parsed[1].temperature_c, None);
        assert_eq!(parsed[1].power_draw_mw, None);
    }

    #[test]
    fn intel_discovery_memory_fallback_respects_reported_units() {
        let mib = json!({"Memory Physical Size": "16384 MiB"});
        let gib = json!({"Memory Physical Size": "16 GiB"});
        let unknown = json!({"Memory Physical Size": "16"});
        assert_eq!(intel_discovery_memory_mib(&mib), Some(16384.0));
        assert_eq!(intel_discovery_memory_mib(&gib), Some(16384.0));
        assert_eq!(intel_discovery_memory_mib(&unknown), None);
    }

    #[test]
    fn intel_csv_identity_fallback_does_not_invent_metrics() {
        let sample = concat!(
            "0, Intel(R) Arc(TM) B580 Graphics, GPU-INTEL-UUID-0, 0000:03:00.0\n",
            "1, Intel(R) Data Center GPU Max, GPU-INTEL-UUID-1, 0000:4d:00.0\n"
        );
        let parsed = parse_intel_identity_csv(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].vendor, "intel");
        assert_eq!(parsed[0].device_index, Some(0));
        assert_eq!(parsed[0].device_id.as_deref(), Some("GPU-INTEL-UUID-0"));
        assert_eq!(parsed[0].pci_bus_id.as_deref(), Some("0000:03:00.0"));
        assert_eq!(parsed[1].device_index, Some(1));
        assert_eq!(parsed[1].device_id.as_deref(), Some("GPU-INTEL-UUID-1"));
        assert_eq!(parsed[0].vram_mib, None);
        assert_eq!(parsed[0].utilization_percent, None);
    }

    #[test]
    fn intel_discovery_json_fallback_keeps_unknown_metrics_unknown() {
        let sample = r#"[
            {
                "Device ID": "0",
                "Device Name": "Intel Arc Pro",
                "SOC UUID": "intel-soc-uuid",
                "PCI BDF Address": "0000:03:00.0",
                "Memory Physical Size": "16384 MiB",
                "UMD Version": "1.2.3",
                "PCIe Generation": "4",
                "PCIe Max Link Width": "16"
            }
        ]"#;
        let parsed = parse_intel_discovery_json(sample);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].vendor, "intel");
        assert_eq!(parsed[0].name, "Intel Arc Pro");
        assert_eq!(parsed[0].device_id.as_deref(), Some("intel-soc-uuid"));
        assert_eq!(parsed[0].device_index, Some(0));
        assert_eq!(parsed[0].pci_bus_id.as_deref(), Some("0000:03:00.0"));
        assert_eq!(parsed[0].vram_mib, Some(16384));
        assert_eq!(parsed[0].driver_version.as_deref(), Some("1.2.3"));
        assert_eq!(parsed[0].pcie_link_gen_max, Some(4));
        assert_eq!(parsed[0].pcie_link_width_max, Some(16));
        assert_eq!(parsed[0].utilization_percent, None);
        assert_eq!(parsed[0].temperature_c, None);
        assert_eq!(parsed[0].power_draw_mw, None);
    }

    #[test]
    fn amd_smi_json_merges_identity_static_and_live_metrics_for_multiple_gpus() {
        let list = r#"{
            "gpu_data": [
                {"gpu": 0, "bdf": "0000:0c:00.0", "uuid": "GPU-AMD-0000"},
                {"gpu": 1, "bdf": "0000:22:00.0", "uuid": "GPU-AMD-0001"}
            ]
        }"#;
        let static_info = r#"{
            "gpu_data": [
                {
                    "gpu": 0,
                    "asic": {
                        "market_name": "AMD Radeon RX 7900 XTX",
                        "asic_serial": "ASIC-0000"
                    },
                    "bus": {
                        "bdf": "0000:0c:00.0",
                        "max_pcie_width": 16,
                        "max_pcie_speed": "32 GT/s",
                        "pcie_interface_version": "Gen 5"
                    },
                    "driver": {"version": "6.19.4"},
                    "vram": {"size": {"value": 24560, "unit": "MB"}}
                },
                {
                    "gpu": 1,
                    "asic": {
                        "market_name": "AMD Instinct MI300X",
                        "asic_serial": "ASIC-0001"
                    },
                    "bus": {
                        "bdf": "0000:22:00.0",
                        "max_pcie_width": 16,
                        "max_pcie_speed": "32 GT/s"
                    },
                    "driver": {"version": "6.19.4"},
                    "vram": {"size": "196300 MB"}
                }
            ]
        }"#;
        let metrics = r#"[
            {
                "gpu": 0,
                "usage": {"gfx_activity": {"value": 27.5, "unit": "%"}},
                "power": {"socket_power": {"value": 220.5, "unit": "W"}},
                "temperature": {"hotspot": {"value": 68, "unit": "C"}},
                "pcie": {
                    "width": 16,
                    "speed": {"value": 32, "unit": "GT/s"}
                },
                "mem_usage": {
                    "total_vram": {"value": 24560, "unit": "MB"},
                    "used_vram": {"value": 4096, "unit": "MB"},
                    "free_vram": {"value": 20464, "unit": "MB"}
                }
            },
            {
                "gpu": 1,
                "usage": {"gfx_activity": "N/A"},
                "power": {"socket_power": "N/A"},
                "temperature": {"hotspot": "N/A"},
                "pcie": {"width": "N/A", "speed": "N/A"},
                "mem_usage": {
                    "used_vram": {"value": 1024, "unit": "MB"},
                    "total_vram": {"value": 196300, "unit": "MB"}
                }
            }
        ]"#;

        let devices = parse_amd_smi_json(list, static_info, metrics);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].vendor, "amd");
        assert_eq!(devices[0].name, "AMD Radeon RX 7900 XTX");
        assert_eq!(devices[0].device_index, Some(0));
        assert_eq!(devices[0].device_id.as_deref(), Some("GPU-AMD-0000"));
        assert_eq!(devices[0].pci_bus_id.as_deref(), Some("0000:0c:00.0"));
        assert_eq!(devices[0].driver_version.as_deref(), Some("6.19.4"));
        assert_eq!(devices[0].vram_mib, Some(24560));
        assert_eq!(devices[0].vram_used_mib, Some(4096));
        assert_eq!(devices[0].vram_free_mib, Some(20464));
        assert_eq!(devices[0].utilization_percent, Some(28));
        assert_eq!(devices[0].temperature_c, Some(68));
        assert_eq!(devices[0].power_draw_mw, Some(220_500));
        assert_eq!(devices[0].pcie_link_width, Some(16));
        assert_eq!(devices[0].pcie_link_gen, Some(5));
        assert_eq!(devices[0].pcie_link_width_max, Some(16));
        assert_eq!(devices[0].pcie_link_gen_max, Some(5));

        assert_eq!(devices[1].name, "AMD Instinct MI300X");
        assert_eq!(devices[1].device_index, Some(1));
        assert_eq!(devices[1].device_id.as_deref(), Some("GPU-AMD-0001"));
        assert_eq!(devices[1].pci_bus_id.as_deref(), Some("0000:22:00.0"));
        assert_eq!(devices[1].vram_mib, Some(196300));
        assert_eq!(devices[1].vram_used_mib, Some(1024));
        assert_eq!(devices[1].vram_free_mib, Some(195276));
        assert_eq!(devices[1].utilization_percent, None);
        assert_eq!(devices[1].temperature_c, None);
        assert_eq!(devices[1].power_draw_mw, None);
        assert_eq!(devices[1].pcie_link_width, None);
        assert_eq!(devices[1].pcie_link_gen, None);
    }

    #[test]
    fn amd_smi_json_accepts_array_metric_reports_and_keeps_unavailable_values_unknown() {
        let list = r#"[{"gpu": 2, "bdf": "0000:43:00.0", "uuid": "GPU-AMD-PARTITION"}]"#;
        let static_info = r#"{
            "gpu_data": [{
                "gpu": 2,
                "asic": {
                    "market_name": "AMD Instinct MI300X",
                    "asic_serial": "ASIC-PARTITION"
                },
                "bus": {"bdf": "0000:43:00.0"}
            }]
        }"#;
        let metrics = r#"[{
            "gpu": 2,
            "usage": {"gfx_activity": {"value": 0, "unit": "%"}},
            "power": {"socket_power": {"value": 0, "unit": "W"}},
            "temperature": {"edge": {"value": 44, "unit": "C"}}
        }]"#;

        let devices = parse_amd_smi_json(list, static_info, metrics);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_index, Some(2));
        assert_eq!(devices[0].device_id.as_deref(), Some("GPU-AMD-PARTITION"));
        assert_eq!(devices[0].pci_bus_id.as_deref(), Some("0000:43:00.0"));
        assert_eq!(devices[0].utilization_percent, Some(0));
        assert_eq!(devices[0].temperature_c, Some(44));
        assert_eq!(devices[0].power_draw_mw, Some(0));
        assert_eq!(devices[0].vram_mib, None);
        assert_eq!(devices[0].vram_used_mib, None);
        assert_eq!(devices[0].vram_free_mib, None);
    }

    #[test]
    fn amd_memory_and_power_unit_parsers_convert_documented_units() {
        assert_eq!(amd_memory_mib(&(24560.0, Some("MB".into()))), Some(24560));
        assert_eq!(amd_memory_mib(&(24.0, Some("GB".into()))), Some(24576));
        assert_eq!(
            amd_memory_mib(&(1_073_741_824.0, Some("B".into()))),
            Some(1024)
        );
        assert_eq!(amd_power_mw(&(220.5, Some("W".into()))), Some(220_500));
        assert_eq!(amd_power_mw(&(750.0, Some("mW".into()))), Some(750));
        assert_eq!(
            amd_pcie_generation(&json!({"value": 32, "unit": "GT/s"})),
            Some(5)
        );
        assert_eq!(
            amd_memory_mib(&amd_number_and_unit(&json!("196300 MB")).unwrap()),
            Some(196300)
        );
        assert_eq!(
            amd_memory_mib(&amd_number_and_unit(&json!("24 GB")).unwrap()),
            Some(24576)
        );
        assert_eq!(
            amd_power_mw(&amd_number_and_unit(&json!("220.5 W")).unwrap()),
            Some(220_500)
        );
        assert_eq!(amd_pcie_generation(&json!("32 GT/s")), Some(5));
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
    fn nvidia_inventory_preserves_stable_uuid_and_pci_identity_for_multiple_gpus() {
        let sample = concat!(
            "0, GPU-aaaaaaaa-1111-2222-3333-444444444444, 00000000:01:00.0, NVIDIA GeForce RTX 3090, 24576, 12000, 12576, 550.90.07, 1, 1, 4, 16, 37, 52, 280.5, 8.6\n",
            "1, GPU-bbbbbbbb-1111-2222-3333-555555555555, 00000000:14:00.0, NVIDIA GeForce RTX 3090, 24576, 8000, 16576, 550.90.07, 1, 4, 4, 16, 72, 68, 310.0, 8.6\n",
        );
        let parsed = parse_nvidia_csv(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].device_index, Some(0));
        assert_eq!(
            parsed[0].device_id.as_deref(),
            Some("GPU-aaaaaaaa-1111-2222-3333-444444444444")
        );
        assert_eq!(parsed[0].pci_bus_id.as_deref(), Some("00000000:01:00.0"));
        assert_eq!(parsed[1].device_index, Some(1));
        assert_eq!(
            parsed[1].device_id.as_deref(),
            Some("GPU-bbbbbbbb-1111-2222-3333-555555555555")
        );
        assert_eq!(parsed[1].pci_bus_id.as_deref(), Some("00000000:14:00.0"));
        assert_ne!(parsed[0].device_id, parsed[1].device_id);
        assert_ne!(parsed[0].pci_bus_id, parsed[1].pci_bus_id);
        assert_eq!(parsed[1].pcie_link_width, Some(4));
    }

    #[test]
    fn nvidia_inventory_keeps_identity_when_compute_capability_is_unavailable() {
        let sample = "0, GPU-cccccccc-1111-2222-3333-666666666666, 00000000:0A:00.0, NVIDIA A100, 40960, 1024, 39936, 550.90.07, 4, 16, 4, 16, 5, 40, 120.0\n";
        let parsed = parse_nvidia_csv(sample);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].device_index, Some(0));
        assert_eq!(
            parsed[0].device_id.as_deref(),
            Some("GPU-cccccccc-1111-2222-3333-666666666666")
        );
        assert_eq!(parsed[0].pci_bus_id.as_deref(), Some("00000000:0A:00.0"));
        assert_eq!(parsed[0].compute_capability, None);
        assert_eq!(parsed[0].vram_mib, Some(40960));
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
                    device_id: None,
                    device_index: None,
                    pci_bus_id: None,
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
