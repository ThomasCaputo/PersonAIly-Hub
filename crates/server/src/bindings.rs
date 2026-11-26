//! Tipi TypeScript dell'API locale, generati dai tipi Rust con `ts-rs`
//! (`packages/api-client/index.d.ts`): l'estensione VS Code li usa invece di
//! tenerne una copia scritta a mano.
//!
//! La prova qui sotto controlla che il file sia aggiornato. Dopo aver cambiato
//! un tipo dell'API lo si rigenera con:
//!
//! ```text
//! UPDATE_BINDINGS=1 cargo test -p app-server bindings
//! ```

use app_core::agents::{Agent, AgentSession, LiveState, PendingRequest, SessionStatus};
use app_core::attachments::{AttachmentInfo, AttachmentKind};
use app_core::chat::GenerationStats;
use app_core::gguf::ThinkingControl;
use app_core::instance::{InstallInfo, InstanceKind, LockInfo};
use app_core::local::runtime::LoadedModel;
use app_core::local::sysmon::{GpuMemory, MemorySnapshot, ProcessGpuMemory};
use app_core::local::{KvCacheType, LoadParams, Placement, SamplingParams};
use app_core::models::ModelInfo;
use app_core::providers::{CloudModel, Provider, ProviderKind};
use app_core::snapshots::RestorePoint;
use app_core::store::{ActionRecord, Conversation, Message};
use app_core::tools::{PermissionLevel, ToolCall, ToolSettings};
use app_core::{Approval, ModelRef, ProviderInfo, RuntimeStatus, SendOutcome};
use ts_rs::{Config, TS};

use crate::api::{
    AnswerBody, BranchBody, ErrorEvent, InfillBody, InfillReply, Info, LoadBody, LocalModels,
    ModelSwitchEvent, Models, RenameBody, ReplyBody, SendBody, TaskBody, TextEvent,
    ToolRequestEvent,
};

const HEADER: &str = "\
// Tipi dell'API locale di PersonAIly Hub, generati dai tipi Rust del nucleo
// (crates/server/src/bindings.rs). Non modificare a mano: dopo aver cambiato
// un tipo, rigenera con `UPDATE_BINDINGS=1 cargo test -p app-server bindings`.
";

/// Il contenuto di `index.d.ts`.
pub fn typescript() -> String {
    // Gli interi di 64 bit viaggiano come numeri JSON.
    let cfg = Config::new().with_large_int("number");
    let decls = [
        // Nucleo e file di blocco
        Info::decl(&cfg),
        InstanceKind::decl(&cfg),
        LockInfo::decl(&cfg),
        InstallInfo::decl(&cfg),
        // Modelli
        Models::decl(&cfg),
        LocalModels::decl(&cfg),
        LoadBody::decl(&cfg),
        InfillBody::decl(&cfg),
        InfillReply::decl(&cfg),
        ModelInfo::decl(&cfg),
        ThinkingControl::decl(&cfg),
        LoadedModel::decl(&cfg),
        LoadParams::decl(&cfg),
        Placement::decl(&cfg),
        KvCacheType::decl(&cfg),
        SamplingParams::decl(&cfg),
        RuntimeStatus::decl(&cfg),
        MemorySnapshot::decl(&cfg),
        GpuMemory::decl(&cfg),
        ProcessGpuMemory::decl(&cfg),
        ProviderInfo::decl(&cfg),
        Provider::decl(&cfg),
        ProviderKind::decl(&cfg),
        CloudModel::decl(&cfg),
        ModelRef::decl(&cfg),
        // Conversazioni e risposte
        Conversation::decl(&cfg),
        RenameBody::decl(&cfg),
        Message::decl(&cfg),
        AttachmentInfo::decl(&cfg),
        AttachmentKind::decl(&cfg),
        ToolCall::decl(&cfg),
        SendBody::decl(&cfg),
        BranchBody::decl(&cfg),
        TextEvent::decl(&cfg),
        ToolRequestEvent::decl(&cfg),
        ModelSwitchEvent::decl(&cfg),
        ErrorEvent::decl(&cfg),
        SendOutcome::decl(&cfg),
        GenerationStats::decl(&cfg),
        AnswerBody::decl(&cfg),
        Approval::decl(&cfg),
        ToolSettings::decl(&cfg),
        PermissionLevel::decl(&cfg),
        ActionRecord::decl(&cfg),
        // Agenti
        Agent::decl(&cfg),
        AgentSession::decl(&cfg),
        RestorePoint::decl(&cfg),
        SessionStatus::decl(&cfg),
        TaskBody::decl(&cfg),
        ReplyBody::decl(&cfg),
        LiveState::decl(&cfg),
        PendingRequest::decl(&cfg),
    ];
    let mut out = String::from(HEADER);
    for decl in decls {
        out.push('\n');
        out.push_str("export ");
        out.push_str(decl.trim());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn i_tipi_typescript_sono_aggiornati() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/api-client/index.d.ts");
        let wanted = super::typescript();
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(&path, &wanted).unwrap();
            return;
        }
        // Git può aver cambiato gli a capo del file sul disco.
        let current = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            current == wanted,
            "packages/api-client/index.d.ts non corrisponde ai tipi Rust: \
             rigeneralo con `UPDATE_BINDINGS=1 cargo test -p app-server bindings`"
        );
    }
}
