//! Lettura della memoria libera e occupata: RAM di sistema, RAM di un
//! processo, VRAM della GPU.

use std::process::Command;

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct MemorySnapshot {
    pub ram_total_bytes: u64,
    pub ram_available_bytes: u64,
    /// `None` se non c'è una GPU NVIDIA o `nvidia-smi` non risponde.
    pub gpu: Option<GpuMemory>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, ts_rs::TS)]
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

/// VRAM della prima GPU NVIDIA.
///
/// Di base dalla libreria del driver (NVML), aperta una volta sola: una
/// lettura costa pochi microsecondi. Se non c'è, da `nvidia-smi`, che però
/// avvia un processo a ogni lettura (circa 70 ms).
///
/// Per ora solo NVIDIA; le altre GPU arriveranno con il supporto a Vulkan.
fn gpu_memory() -> Option<GpuMemory> {
    #[cfg(windows)]
    if let Some(nvml) = nvml::get() {
        return nvml.memory();
    }
    gpu_memory_from_smi()
}

/// Accesso diretto a `nvml.dll`, la libreria che il driver NVIDIA installa in
/// `System32` (la stessa che usa `nvidia-smi`). Solo le quattro funzioni che
/// servono, caricate a mano: niente dipendenze in più.
#[cfg(windows)]
mod nvml {
    use std::ffi::{c_char, c_int, c_uint, c_void};
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };

    use super::GpuMemory;

    type Device = *mut c_void;
    type InitFn = unsafe extern "C" fn() -> c_int;
    type HandleFn = unsafe extern "C" fn(c_uint, *mut Device) -> c_int;
    type NameFn = unsafe extern "C" fn(Device, *mut c_char, c_uint) -> c_int;
    type MemoryFn = unsafe extern "C" fn(Device, *mut Memory) -> c_int;
    /// Come `GetProcAddress` restituisce una funzione (`FARPROC` senza `Option`).
    type Symbol = unsafe extern "system" fn() -> isize;

    /// `nvmlMemory_t`: in byte.
    #[repr(C)]
    #[derive(Default)]
    struct Memory {
        total: u64,
        free: u64,
        used: u64,
    }

    pub(super) struct Nvml {
        device: usize,
        name: String,
        memory_fn: MemoryFn,
    }

    impl Nvml {
        pub(super) fn memory(&self) -> Option<GpuMemory> {
            let mut memory = Memory::default();
            // SAFETY: `device` viene da `nvmlDeviceGetHandleByIndex_v2` e la
            // libreria resta caricata per tutta la vita del processo.
            let status = unsafe { (self.memory_fn)(self.device as Device, &mut memory) };
            (status == 0).then(|| GpuMemory {
                name: self.name.clone(),
                total_bytes: memory.total,
                used_bytes: memory.used,
                free_bytes: memory.free,
            })
        }
    }

    /// La libreria aperta, o `None` se non c'è una GPU NVIDIA: si prova una
    /// volta sola.
    pub(super) fn get() -> Option<&'static Nvml> {
        static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
        NVML.get_or_init(|| unsafe { open() }).as_ref()
    }

    unsafe fn open() -> Option<Nvml> {
        let file: Vec<u16> = "nvml.dll".encode_utf16().chain([0]).collect();
        // Solo da System32: mai una nvml.dll messa accanto all'eseguibile.
        let module = unsafe {
            LoadLibraryExW(
                file.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return None;
        }
        let symbol = |name: &[u8]| unsafe { GetProcAddress(module, name.as_ptr()) };
        let init = symbol(b"nvmlInit_v2\0")?;
        let handle = symbol(b"nvmlDeviceGetHandleByIndex_v2\0")?;
        let name_fn = symbol(b"nvmlDeviceGetName\0")?;
        let memory = symbol(b"nvmlDeviceGetMemoryInfo\0")?;
        // SAFETY: firme dalla documentazione di NVML (nvml.h).
        let (init, handle, name_fn, memory_fn) = unsafe {
            (
                std::mem::transmute::<Symbol, InitFn>(init),
                std::mem::transmute::<Symbol, HandleFn>(handle),
                std::mem::transmute::<Symbol, NameFn>(name_fn),
                std::mem::transmute::<Symbol, MemoryFn>(memory),
            )
        };
        if unsafe { init() } != 0 {
            return None;
        }
        let mut device: Device = std::ptr::null_mut();
        if unsafe { handle(0, &mut device) } != 0 {
            return None;
        }
        let mut buffer = [0 as c_char; 96];
        let name = if unsafe { name_fn(device, buffer.as_mut_ptr(), buffer.len() as c_uint) } == 0 {
            let bytes: Vec<u8> = buffer
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            "NVIDIA GPU".to_string()
        };
        Some(Nvml {
            device: device as usize,
            name,
            memory_fn,
        })
    }
}

/// VRAM letta da `nvidia-smi`: la riserva quando NVML non si apre.
fn gpu_memory_from_smi() -> Option<GpuMemory> {
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

    /// Su un PC con GPU NVIDIA le due letture devono dire la stessa cosa.
    #[cfg(windows)]
    #[test]
    fn nvml_e_nvidia_smi_concordano() {
        let Some(smi) = gpu_memory_from_smi() else {
            return; // niente GPU NVIDIA: nulla da confrontare
        };
        // Dove funziona nvidia-smi c'è anche la sua libreria.
        let nvml = nvml::get()
            .and_then(|n| n.memory())
            .expect("NVML non si apre");
        assert_eq!(nvml.name, smi.name);
        // nvidia-smi arrotonda ai MiB.
        assert_eq!(
            nvml.total_bytes / (1024 * 1024),
            smi.total_bytes / (1024 * 1024)
        );
    }

    #[test]
    fn uscita_inattesa_non_fa_danni() {
        assert!(parse_nvidia_smi("").is_none());
        assert!(parse_nvidia_smi("errore del driver").is_none());
    }
}
