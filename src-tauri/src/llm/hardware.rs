use serde::Serialize;
use sysinfo::System;

const BYTES_PER_GB: f64 = (1u64 << 30) as f64;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub ram_gb: u64,
    pub cpu: String,
    pub cores: usize,
    pub os: String,
    pub arch: String,
    /// How llama.cpp accelerates inference in this build.
    pub accelerator: String,
}

pub fn detect() -> Hardware {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_cpu_all();
    let cpu = system.cpus().first().map(|cpu| cpu.brand().trim().to_owned()).filter(|brand| !brand.is_empty());
    let accelerator = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "Apple Silicon GPU (Metal, unified memory)"
    } else {
        "CPU"
    };
    Hardware {
        ram_gb: (system.total_memory() as f64 / BYTES_PER_GB).round() as u64,
        cpu: cpu.unwrap_or_else(|| "Unknown CPU".into()),
        cores: system.cpus().len(),
        os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
        arch: std::env::consts::ARCH.into(),
        accelerator: accelerator.into(),
    }
}
