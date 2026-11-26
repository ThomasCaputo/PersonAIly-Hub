//! Backup, registri e informazioni diagnostiche.

use crate::*;

impl Core {
    /// Salva conversazioni, agenti, impostazioni e preset in un file `.zip`.
    pub fn create_backup(&self, dest: &Path) -> Result<backup::BackupInfo> {
        // L'archivio si blocca solo per la copia, non per la compressione.
        backup::create(|copy| self.store().vacuum_into(copy), &self.data_dir, dest)
    }

    /// Prepara il ripristino di un backup, che si applica al prossimo avvio:
    /// l'app va riavviata subito dopo.
    pub fn stage_restore(&self, source: &Path) -> Result<backup::BackupInfo> {
        backup::stage(&self.data_dir, source)
    }

    /// Cartella dei registri, da aprire dall'interfaccia.
    pub fn logs_dir(&self) -> PathBuf {
        log::dir(&self.data_dir)
    }

    /// Testo da allegare a una segnalazione: versioni, hardware, motore,
    /// modello caricato, quante cose sono configurate e le ultime righe dei
    /// registri. Niente chiavi, token, indirizzi dei provider, nomi di
    /// cartelle né conversazioni; la cartella utente e il nome utente sono
    /// sostituiti. Va eseguito fuori dal ciclo asincrono (misura la GPU).
    pub fn diagnostics(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let mut line = |label: &str, value: String| {
            let _ = writeln!(out, "{label}: {value}");
        };
        line("PersonAIly Hub", version().to_string());
        line(
            "Archivio",
            format!("schema {}", self.schema_version().unwrap_or(0)),
        );
        line(
            "Sistema",
            format!(
                "{} ({})",
                sysinfo::System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.into()),
                std::env::consts::ARCH
            ),
        );
        if let Ok(mut sys) = sysinfo::System::new() {
            sys.refresh_cpu_all();
            if let Some(cpu) = sys.cpus().first() {
                line(
                    "Processore",
                    format!("{} ({} thread)", cpu.brand().trim(), sys.cpus().len()),
                );
            }
        }
        let memory = sysmon::snapshot();
        line(
            "RAM",
            format!(
                "{} GB, liberi {} GB",
                gib(memory.ram_total_bytes),
                gib(memory.ram_available_bytes)
            ),
        );
        line(
            "GPU",
            match &memory.gpu {
                Some(gpu) => format!(
                    "{}, {} GB, liberi {} GB",
                    gpu.name,
                    gib(gpu.total_bytes),
                    gib(gpu.free_bytes)
                ),
                None => "nessuna GPU NVIDIA rilevata".to_string(),
            },
        );
        let engines: Vec<String> = local::runtime::installed_engines(&self.data_dir)
            .into_iter()
            .map(|e| e.dir_name)
            .collect();
        line(
            "llama.cpp",
            format!(
                "installate [{}], scelta {:?}, prevista {}",
                engines.join(", "),
                self.engine_variant(),
                local::LLAMA_CPP_BUILD
            ),
        );
        let loaded = match self.runtime.try_lock() {
            Ok(mut runtime) => match runtime.loaded() {
                Some(m) => format!(
                    "{} con {} ({:?}, contesto {}, cache {:?}, immagini {})",
                    m.model_name,
                    m.engine,
                    m.params.placement,
                    m.params.context_length,
                    m.params.kv_cache_type,
                    m.vision
                ),
                None => "nessuno".to_string(),
            },
            Err(_) => "caricamento in corso".to_string(),
        };
        line("Modello locale", loaded);
        let providers: Vec<String> = self
            .providers()
            .unwrap_or_default()
            .iter()
            .map(|p| {
                format!(
                    "{:?}{}",
                    p.provider.kind,
                    if p.has_key { "" } else { " senza chiave" }
                )
            })
            .collect();
        line("Provider cloud", format!("[{}]", providers.join(", ")));
        line("Agenti", self.agents().map_or(0, |a| a.len()).to_string());
        line(
            "Server MCP",
            self.mcp_servers().map_or(0, |s| s.len()).to_string(),
        );
        let prefs = self.preferences();
        line(
            "Preferenze",
            format!(
                "area di notifica {}, scaricamento dopo {} min",
                prefs.keep_running, prefs.idle_unload_minutes
            ),
        );
        let _ = writeln!(out, "\n--- core.log (ultime righe) ---");
        let _ = writeln!(out, "{}", log::tail(&self.data_dir, 150));
        let server_log = log::dir(&self.data_dir).join("llama-server.log");
        if let Ok(bytes) = std::fs::read(&server_log) {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(40);
            let _ = writeln!(out, "\n--- llama-server.log (ultime righe) ---");
            let _ = writeln!(out, "{}", lines[start..].join("\n"));
        }
        log::redact(&out)
    }
}
