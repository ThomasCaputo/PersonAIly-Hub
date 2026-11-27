//! Nucleo senza interfaccia (VS-5): espone l'API locale anche ad app chiusa.
//!
//! Lo avvia l'estensione VS Code quando non trova un nucleo attivo. Se l'app
//! desktop viene aperta, gli chiede di chiudersi e prende il suo posto.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use app_core::Core;
use app_core::instance::{self, InstanceKind};

const HELP: &str = "\
Nucleo di PersonAIly Hub senza interfaccia: espone l'API locale su 127.0.0.1.

Uso: app-server [opzioni]

  --data-dir <cartella>   cartella dati (predefinita: quella dell'app desktop)
  --port <numero>         porta preferita (predefinita: 41241; 0 = scelta dal sistema)
  --idle-minutes <n>      si chiude dopo n minuti senza richieste né lavoro
                          in corso (predefinito: 30; 0 = mai)
  --version               stampa la versione
  --help                  stampa questo aiuto

Porta e token del nucleo in esecuzione sono nel file core.lock della cartella dati.";

struct Options {
    data_dir: Option<PathBuf>,
    port: u16,
    idle_minutes: u64,
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut options = Options {
        data_dir: None,
        port: app_server::DEFAULT_PORT,
        idle_minutes: 30,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("manca il valore di {name}"))
        };
        match arg.as_str() {
            "--data-dir" => options.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--port" => {
                options.port = value("--port")?
                    .parse()
                    .map_err(|_| "--port vuole un numero da 0 a 65535".to_string())?
            }
            "--idle-minutes" => {
                options.idle_minutes = value("--idle-minutes")?
                    .parse()
                    .map_err(|_| "--idle-minutes vuole un numero".to_string())?
            }
            "--version" => {
                println!("app-server {}", app_core::version());
                return Ok(None);
            }
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(None);
            }
            other => return Err(format!("opzione sconosciuta: {other} (vedi --help)")),
        }
    }
    Ok(Some(options))
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("errore: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let Some(options) = parse_args()? else {
        return Ok(());
    };
    let data_dir = options
        .data_dir
        .or_else(instance::default_data_dir)
        .ok_or("cartella dati non trovata: indicala con --data-dir")?;

    // Un solo nucleo attivo: se c'è già, i client useranno quello.
    if let Some(lock) = instance::running_instance(&data_dir) {
        let who = match lock.kind {
            InstanceKind::App => "dell'app desktop",
            InstanceKind::Server => "senza interfaccia",
        };
        println!(
            "Il nucleo {who} è già attivo su http://127.0.0.1:{}",
            lock.port
        );
        return Ok(());
    }

    app_core::log::init(&data_dir, "app-server");
    app_core::log::install_panic_hook();
    let core = Arc::new(Core::new(&data_dir).map_err(|e| {
        app_core::log::error(format!("avvio del nucleo non riuscito: {e}"));
        e.to_string()
    })?);
    let api =
        app_server::start(core.clone(), &data_dir, InstanceKind::Server, options.port).await?;
    println!("API locale su http://127.0.0.1:{}", api.port());
    println!("Cartella dati: {}", data_dir.display());
    // Scaricamento del modello dopo un periodo di inattività (LO-8).
    tokio::spawn(core.clone().maintenance());

    let idle_limit =
        (options.idle_minutes > 0).then(|| Duration::from_secs(options.idle_minutes * 60));
    let mut tick = tokio::time::interval(Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("Chiusura richiesta da tastiera.");
                break;
            }
            _ = api.quit_requested() => {
                println!("Chiusura richiesta da un client.");
                break;
            }
            _ = tick.tick() => {
                if let Some(limit) = idle_limit
                    && api.idle_for() >= limit
                    && !core.is_busy()
                    && !core.has_scheduled_work()
                {
                    println!("Nessuna richiesta da {} minuti: mi chiudo.", options.idle_minutes);
                    break;
                }
            }
        }
    }

    // Prima si toglie il file di blocco, così chi arriva non trova un nucleo
    // che sta per sparire; poi si ferma il lavoro e si libera la memoria.
    app_core::log::info("chiusura di app-server");
    api.stop().await;
    core.stop_all_sessions();
    core.stop_generation();
    core.unload_model().await;
    Ok(())
}
