//! Misura la memoria usata da un modello caricato (vedi ADR 0003).
//!
//! ```text
//! cargo run -p app-core --example misura -- <cartella dati> <modello.gguf> <vram|ram> [load-mode] [contesto]
//! ```
//!
//! Stampa RAM e VRAM prima del caricamento, dopo il caricamento, dopo una
//! risposta e dopo lo scaricamento.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use app_core::chat::{self, ChatMessage};
use app_core::local::runtime::LocalRuntime;
use app_core::local::sysmon::{self, MemorySnapshot};
use app_core::local::{KvCacheType, LoadParams, Placement, SamplingParams, estimate_memory};
use app_core::models;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const MIB: f64 = 1024.0 * 1024.0;

fn mib(bytes: u64) -> f64 {
    bytes as f64 / MIB
}

struct Reading {
    memory: MemorySnapshot,
    working_set: u64,
    committed: u64,
    gpu: Option<sysmon::ProcessGpuMemory>,
}

fn read(pid: Option<u32>) -> Reading {
    let (working_set, committed) = pid
        .and_then(|pid| {
            let pid = Pid::from_u32(pid);
            let mut sys = System::new().ok()?;
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_memory(),
            );
            sys.process(pid).map(|p| (p.memory(), p.virtual_memory()))
        })
        .unwrap_or((0, 0));
    Reading {
        memory: sysmon::snapshot(),
        working_set,
        committed,
        gpu: pid.and_then(sysmon::process_gpu_memory),
    }
}

fn print(label: &str, now: &Reading, base: &Reading) {
    let vram = |r: &Reading| r.memory.gpu.as_ref().map(|g| g.used_bytes).unwrap_or(0);
    println!(
        "{label:<22} VRAM {:>6.0} MiB ({:+7.0})   RAM di sistema occupata {:+7.0} MiB   processo: in RAM {:>6.0} MiB, riservata {:>6.0} MiB",
        mib(vram(now)),
        mib(vram(now)) - mib(vram(base)),
        mib(base.memory.ram_available_bytes) - mib(now.memory.ram_available_bytes),
        mib(now.working_set),
        mib(now.committed),
    );
    if let Some(gpu) = now.gpu {
        println!(
            "{:<22} memoria grafica del processo: dedicata {:>6.0} MiB, condivisa (RAM usata come VRAM) {:>6.0} MiB",
            "",
            mib(gpu.dedicated_bytes),
            mib(gpu.shared_bytes),
        );
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("uso: misura <cartella dati> <modello.gguf> <vram|ram> [load-mode] [contesto]");
        std::process::exit(2);
    }
    let params = LoadParams {
        placement: if args[2] == "ram" {
            Placement::Ram
        } else {
            Placement::Vram
        },
        load_mode: args.get(3).filter(|m| *m != "-").cloned(),
        context_length: args.get(4).and_then(|c| c.parse().ok()).unwrap_or(8192),
        kv_cache_type: match args.get(5).map(String::as_str) {
            Some("f16") => KvCacheType::F16,
            Some("q4_0") => KvCacheType::Q4_0,
            _ => KvCacheType::Q8_0,
        },
        ..Default::default()
    };

    let model = models::inspect(std::path::Path::new(&args[1]));
    let estimate = estimate_memory(&model, &params);
    println!(
        "{} — {:?}, lettura file \"{}\", contesto {}",
        model.name,
        params.placement,
        params.effective_load_mode(),
        params.context_length
    );
    println!(
        "stima: pesi {:.0} + cache KV {:.0} + margine {:.0} = {:.0} MiB",
        mib(estimate.weights_bytes),
        mib(estimate.kv_cache_bytes),
        mib(estimate.overhead_bytes),
        mib(estimate.total_bytes)
    );

    let http = reqwest::Client::builder().no_proxy().build()?;
    let mut runtime = LocalRuntime::new(PathBuf::from(&args[0]), http.clone());

    let base = read(None);
    print("prima", &base, &base);

    let start = Instant::now();
    let loaded = runtime.load(&args[1], &model.name, &params).await?;
    println!("caricato in {:.1} s", start.elapsed().as_secs_f64());
    print("appena caricato", &read(Some(loaded.pid)), &base);

    tokio::time::sleep(Duration::from_secs(5)).await;
    print("dopo 5 s", &read(Some(loaded.pid)), &base);

    let messages = [ChatMessage {
        role: "user".to_string(),
        content: "Spiega in circa 150 parole che cos'è la cache KV di un modello linguistico."
            .to_string(),
    }];
    let sampling = SamplingParams {
        max_tokens: Some(300),
        ..Default::default()
    };
    let outcome = chat::stream_chat(
        &http,
        &format!("http://127.0.0.1:{}/v1/chat/completions", loaded.port),
        None,
        &chat::request_body(&messages, &sampling, None, chat::Flavor::Llama),
        Arc::new(tokio::sync::Notify::new()),
        |_| {},
    )
    .await?;
    println!(
        "risposta: {} token a {:.1} token/s — \"{}…\"",
        outcome.stats.completion_tokens.unwrap_or(0),
        outcome.stats.tokens_per_second.unwrap_or(0.0),
        outcome
            .text
            .chars()
            .take(60)
            .collect::<String>()
            .replace('\n', " ")
    );
    print("dopo una risposta", &read(Some(loaded.pid)), &base);

    runtime.unload().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    print("dopo lo scaricamento", &read(None), &base);
    Ok(())
}
