//! Prove dell'API locale su una cartella dati temporanea, senza modelli.

use std::sync::Arc;

use app_core::Core;
use app_core::instance::{self, InstanceKind};
use serde_json::{Value, json};

const TOKEN: &str = "token-di-prova";

struct Fixture {
    dir: std::path::PathBuf,
    base: String,
    http: reqwest::Client,
    api: Option<app_server::ApiHandle>,
}

impl Fixture {
    async fn new(name: &str, kind: InstanceKind) -> Self {
        let dir = std::env::temp_dir().join(format!("api-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let core = Arc::new(Core::new(&dir).unwrap());
        let api = app_server::start_with_token(core, &dir, kind, 0, TOKEN.into())
            .await
            .unwrap();
        Self {
            base: format!("http://127.0.0.1:{}", api.port()),
            http: reqwest::Client::builder().no_proxy().build().unwrap(),
            api: Some(api),
            dir,
        }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{}{path}", self.base))
            .bearer_auth(TOKEN)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{}{path}", self.base))
            .bearer_auth(TOKEN)
    }

    async fn close(mut self) {
        if let Some(api) = self.api.take() {
            api.stop().await;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn il_token_e_obbligatorio() {
    let f = Fixture::new("token", InstanceKind::App).await;

    let none = f
        .http
        .get(format!("{}/api/info", f.base))
        .send()
        .await
        .unwrap();
    assert_eq!(none.status(), 401);
    let body: Value = none.json().await.unwrap();
    assert_eq!(body["error"]["type"], "authentication_error");

    let wrong = f
        .http
        .get(format!("{}/api/info", f.base))
        .bearer_auth("altro")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);

    // Anche gli indirizzi sconosciuti chiedono il token: non rivelano nulla.
    let unknown = f
        .http
        .get(format!("{}/segreto", f.base))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 401);

    let ok = f.get("/api/info").send().await.unwrap();
    assert_eq!(ok.status(), 200);
    let info: Value = ok.json().await.unwrap();
    assert_eq!(info["kind"], "app");
    f.close().await;
}

#[tokio::test]
async fn le_pagine_web_sono_respinte() {
    let f = Fixture::new("origin", InstanceKind::App).await;
    // Una pagina web che conoscesse il token resta comunque fuori.
    let from_page = f
        .get("/api/info")
        .header("Origin", "https://esempio.com")
        .send()
        .await
        .unwrap();
    assert_eq!(from_page.status(), 403);

    // DNS rebinding: un nome che punta a 127.0.0.1 ma non è "localhost".
    let rebinding = f
        .get("/api/info")
        .header("Host", "attacco.esempio.com")
        .send()
        .await
        .unwrap();
    assert_eq!(rebinding.status(), 403);
    f.close().await;
}

#[tokio::test]
async fn file_di_blocco_e_chiusura() {
    let f = Fixture::new("lock", InstanceKind::Server).await;
    let lock = instance::read_lock(&f.dir).expect("file di blocco scritto");
    assert_eq!(lock.token, TOKEN);
    assert_eq!(lock.kind, InstanceKind::Server);
    assert_eq!(format!("http://127.0.0.1:{}", lock.port), f.base);

    let shutdown = f.post("/api/shutdown").send().await.unwrap();
    assert_eq!(shutdown.status(), 202);
    let api = f.api.as_ref().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), api.quit_requested())
        .await
        .expect("chiusura segnalata");

    let dir = f.dir.clone();
    f.close().await;
    assert!(instance::read_lock(&dir).is_none(), "file di blocco tolto");
}

#[tokio::test]
async fn l_app_non_si_chiude_da_fuori() {
    let f = Fixture::new("app-shutdown", InstanceKind::App).await;
    let shutdown = f.post("/api/shutdown").send().await.unwrap();
    assert_eq!(shutdown.status(), 409);
    f.close().await;
}

#[tokio::test]
async fn conversazioni() {
    let f = Fixture::new("conv", InstanceKind::App).await;
    let created: Value = f
        .post("/api/conversations")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_i64().unwrap();

    let renamed = f
        .http
        .patch(format!("{}/api/conversations/{id}", f.base))
        .bearer_auth(TOKEN)
        .json(&json!({ "title": "Da VS Code" }))
        .send()
        .await
        .unwrap();
    assert_eq!(renamed.status(), 204);

    let list: Value = f
        .get("/api/conversations")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list[0]["title"], "Da VS Code");

    let messages: Value = f
        .get(&format!("/api/conversations/{id}/messages"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(messages, json!([]));

    // Senza un modello caricato l'errore arriva subito, non a flusso aperto.
    let send = f
        .post(&format!("/api/conversations/{id}/messages"))
        .json(&json!({ "text": "Ciao", "model": { "kind": "local" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(send.status(), 400);
    let body: Value = send.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Nessun modello locale")
    );
    f.close().await;
}

#[tokio::test]
async fn endpoint_compatibile_openai() {
    let f = Fixture::new("openai", InstanceKind::App).await;
    let models: Value = f
        .get("/v1/models")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["object"], "list");
    assert_eq!(models["data"], json!([]));

    let unknown = f
        .post("/v1/chat/completions")
        .json(&json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "Ciao" }] }))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 404);

    let no_model = f
        .post("/v1/chat/completions")
        .json(&json!({ "model": "local", "stream": true, "messages": [{ "role": "user", "content": "Ciao" }] }))
        .send()
        .await
        .unwrap();
    assert_eq!(no_model.status(), 400);
    f.close().await;
}
