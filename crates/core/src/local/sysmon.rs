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

/// Memoria grafica usata da un singolo processo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessGpuMemory {
    /// Memoria sulla scheda video (VRAM vera).
    pub dedicated_bytes: u64,
    /// Memoria "condivisa": è RAM di sistema usata come se fosse VRAM. Un valore
    /// alto significa che il driver sta riversando la VRAM in RAM.
    pub shared_bytes: u64,
}

/// Memoria grafica dedicata e condivisa di un processo, dagli stessi
/// contatori che usa Gestione attività. `None` se non sono disponibili.
#[cfg(windows)]
pub fn process_gpu_memory(pid: u32) -> Option<ProcessGpuMemory> {
    use windows_sys::Win32::System::Performance::{
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_MORE_DATA, PdhAddEnglishCounterW,
        PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    };

    /// Somma i valori di tutte le istanze di un contatore (una per scheda video).
    unsafe fn sum(counter: *mut core::ffi::c_void) -> Option<u64> {
        let mut size = 0u32;
        let mut count = 0u32;
        // La prima chiamata dice quanto spazio serve.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                &mut size,
                &mut count,
                std::ptr::null_mut(),
            )
        };
        if status != PDH_MORE_DATA || size == 0 {
            return None;
        }
        // Vettore di u64 per garantire l'allineamento richiesto dalla struttura.
        let mut buffer = vec![0u64; size as usize / 8 + 1];
        let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let status = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut size, &mut count, items)
        };
        if status != 0 {
            return None;
        }
        let mut total = 0u64;
        for i in 0..count as usize {
            let item = unsafe { &*items.add(i) };
            if item.FmtValue.CStatus == 0 {
                total += unsafe { item.FmtValue.Anonymous.largeValue }.max(0) as u64;
            }
        }
        Some(total)
    }

    let wide = |text: String| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    // Nomi in inglese: funzionano con qualsiasi lingua di Windows.
    let dedicated_path = wide(format!(r"\GPU Process Memory(pid_{pid}_*)\Dedicated Usage"));
    let shared_path = wide(format!(r"\GPU Process Memory(pid_{pid}_*)\Shared Usage"));

    unsafe {
        let mut query = std::ptr::null_mut();
        if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
            return None;
        }
        let mut dedicated = std::ptr::null_mut();
        let mut shared = std::ptr::null_mut();
        let result = if PdhAddEnglishCounterW(query, dedicated_path.as_ptr(), 0, &mut dedicated)
            == 0
            && PdhAddEnglishCounterW(query, shared_path.as_ptr(), 0, &mut shared) == 0
            && PdhCollectQueryData(query) == 0
        {
            match (sum(dedicated), sum(shared)) {
                (Some(dedicated_bytes), Some(shared_bytes)) => Some(ProcessGpuMemory {
                    dedicated_bytes,
                    shared_bytes,
                }),
                _ => None,
            }
        } else {
            None
        };
        PdhCloseQuery(query);
        result
    }
}

#[cfg(not(windows))]
pub fn process_gpu_memory(_pid: u32) -> Option<ProcessGpuMemory> {
    None
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
