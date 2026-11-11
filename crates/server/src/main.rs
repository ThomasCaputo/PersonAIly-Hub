//! Nucleo senza interfaccia.
//!
//! Per ora stampa solo la versione: l'API locale arriva con la Tappa 5
//! (vedi `docs/roadmap.md`).

fn main() {
    println!("nucleo {}", app_core::version());
    println!("L'API locale non è ancora disponibile.");
}
