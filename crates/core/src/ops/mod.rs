//! I metodi di [`Core`](crate::Core), divisi per argomento: ogni file
//! aggiunge un blocco `impl Core`.

pub(crate) mod conversation;
pub(crate) mod diagnostics;
pub(crate) mod documents;
pub(crate) mod engine;
pub(crate) mod library;
pub(crate) mod mcp_oauth;
pub(crate) mod mcp_servers;
pub(crate) mod plugins;
pub(crate) mod providers;
pub(crate) mod rounds;
pub(crate) mod schedules;
pub(crate) mod sessions;
pub(crate) mod settings;
pub(crate) mod skills;
