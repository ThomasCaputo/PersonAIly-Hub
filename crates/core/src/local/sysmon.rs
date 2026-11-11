//! Lettura della memoria libera e occupata: RAM di sistema, RAM di un
//! processo, VRAM della GPU.

use std::process::Command;

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySnapshot {
    pub ram_total_bytes: u64,
    pub ram_available_bytes: u64,
    /// `None` se non c'è una GPU NVIDIA o `nvidia-smi` non risponde.
    pub gpu: Option<GpuMemory>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMemory {
    pub name: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

pub fn snapshot() -> MemorySnapshot {
    let Ok(mut sys) = System::new() else {
        return MemorySnapshot::default();
    };
    sys.refresh_memory();
    MemorySnapshot {
        ram_total_bytes: sys.total_memory(),
        ram_available_bytes: sys.available_memory(),
        gpu: gpu_memory(),
    }
}

/// RAM fisica occupata da un processo, in byte.
pub fn process_ram(pid: u32) -> Option<u64> {
    let pid = Pid::from_u32(pid);
    let mut sys = System::new().ok()?;
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    sys.process(pid).map(|p| p.memory())
}

/// VRAM della prima GPU NVIDIA, letta da `nvidia-smi`.
///
/// Per ora solo NVIDIA; le altre GPU arriveranno con il supporto a Vulkan.
fn gpu_memory() -> Option<GpuMemory> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,memory.total,memory.used,memory.free",
        "--format=csv,noheader,nounits",
    ]);
    hide_window(&mut cmd);

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_nvidia_smi(&String::from_utf8_lossy(&output.stdout))
}

fn parse_nvidia_smi(text: &str) -> Option<GpuMemory> {
    const MIB: u64 = 1024 * 1024;
    let line = text.lines().next()?;
    let mut parts = line.split(',').map(str::trim);
    let name = parts.next()?.to_string();
    let mut next_mib = || parts.next()?.parse::<u64>().ok().map(|n| n * MIB);
    Some(GpuMemory {
        name,
        total_bytes: next_mib()?,
        used_bytes: next_mib()?,
        free_bytes: next_mib()?,
    })
}

/// Su Windows evita che si apra una finestra di console a ogni lettura.
fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(super::runtime::CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreta_l_uscita_di_nvidia_smi() {
        let gpu = parse_nvidia_smi("NVIDIA GeForce RTX 3060, 12288, 1051, 11082\n").unwrap();
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 3060");
        assert_eq!(gpu.total_bytes, 12288 * 1024 * 1024);
        assert_eq!(gpu.used_bytes, 1051 * 1024 * 1024);
    }

    #[test]
    fn uscita_inattesa_non_fa_danni() {
        assert!(parse_nvidia_smi("").is_none());
        assert!(parse_nvidia_smi("errore del driver").is_none());
    }
}
