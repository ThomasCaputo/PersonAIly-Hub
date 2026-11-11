//! Installa llama.cpp in una cartella, per provare download, controllo ed estrazione.
//!
//! ```text
//! cargo run -p app-core --example installa -- <cartella dati> <cuda|vulkan|cpu>
//! ```

use app_core::Core;
use app_core::local::install::{Phase, Variant};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("uso: installa <cartella dati> <cuda|vulkan|cpu>");
        std::process::exit(2);
    }
    let variant = match args[1].as_str() {
        "cuda" => Variant::Cuda,
        "vulkan" => Variant::Vulkan,
        _ => Variant::Cpu,
    };

    let core = Core::new(std::path::Path::new(&args[0]))?;
    println!("prima: {:?}", core.engine_status().await.installed);

    let mut last_percent = 0;
    core.install_engine(variant, |p| {
        let percent = p.done_bytes * 100 / p.total_bytes.max(1);
        if p.phase == Phase::Extract {
            println!("estrazione…");
        } else if percent >= last_percent + 25 {
            last_percent = percent;
            println!("scaricato {percent}%");
        }
    })
    .await?;

    println!("dopo: {:?}", core.engine_status().await.installed);
    Ok(())
}
