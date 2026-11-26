//! Accesso OAuth ai server MCP remoti (ADR 0024): scoperta del server di
//! autorizzazione, registrazione dinamica, PKCE, ritorno su `127.0.0.1`,
//! scambio e rinnovo dei token.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::{Error, Result};
use crate::mcp::{EnvVault, McpServerConfig, check_url};

/// Nel portachiavi: i token, divisi in pezzi (`oauth:access#0`, `#1`…).
pub const ACCESS_KEY: &str = "oauth:access";
pub const REFRESH_KEY: &str = "oauth:refresh";
/// Una voce del Gestore credenziali tiene al massimo 2560 byte (1280
/// caratteri in UTF-16): i token più lunghi si dividono.
const CHUNK_CHARS: usize = 1000;
/// Pezzi vecchi da togliere quando un token si accorcia.
const MAX_CHUNKS: usize = 16;
/// Quanto si aspetta che l'utente completi l'accesso nel browser.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Un token che scade entro questo margine si rinnova prima di usarlo.
const EXPIRY_MARGIN: u64 = 60;

/// Dati non segreti di un accesso, salvati nell'archivio.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OAuthRecord {
    pub token_endpoint: String,
    pub client_id: String,
    /// L'indirizzo del server MCP per cui valgono i token (RFC 8707).
    pub resource: String,
    /// Secondi dal 1970 in cui scade il token di accesso, se il servizio lo dice.
    pub expires_at: Option<u64>,
    /// Il segreto del client ottenuto con la registrazione dinamica sta
    /// nel portachiavi; qui solo se c'è.
    pub registered_secret: bool,
}

/// Lo stato dell'accesso, per l'interfaccia.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatus {
    pub signed_in: bool,
    pub expires_at: Option<u64>,
    /// `true` se scaduto il token si rinnova da solo.
    pub refreshable: bool,
}

/// Dove mandare l'utente e dove scambiare il codice.
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoints {
    pub authorization: String,
    pub token: String,
    pub registration: Option<String>,
    pub resource: String,
    pub scope: Option<String>,
}

/// I token ricevuti dal servizio.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenSet {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_in: Option<u64>,
}

/// Un accesso aperto nel browser, in attesa del ritorno.
pub struct PendingLogin {
    pub listener: TcpListener,
    pub state: String,
    pub verifier: String,
    pub redirect_uri: String,
    pub token_endpoint: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    /// `true` se il client l'ha registrato l'app (registrazione dinamica).
    pub registered: bool,
    pub resource: String,
}

/// Ciò che l'interfaccia apre nel browser.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub url: String,
    /// L'host della pagina di accesso, da mostrare prima di aprirla.
    pub host: String,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `true` se il token va rinnovato prima di usarlo.
pub fn expired(record: &OAuthRecord) -> bool {
    record
        .expires_at
        .is_some_and(|at| now() + EXPIRY_MARGIN >= at)
}

fn random_token(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| Error::msg(format!("generatore casuale: {e}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

/// La sfida PKCE `S256` di un verificatore.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Legge `resource_metadata` e `scope` da un'intestazione `WWW-Authenticate`.
pub fn parse_www_authenticate(header: &str) -> (Option<String>, Option<String>) {
    let value_of = |key: &str| -> Option<String> {
        let mut rest = header;
        loop {
            let at = rest.find(key)?;
            let before = rest[..at].chars().last();
            let after = &rest[at + key.len()..];
            // Il nome intero, non un pezzo di un altro (`xscope=`).
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                rest = after;
                continue;
            }
            let after = after.trim_start();
            let Some(after) = after.strip_prefix('=') else {
                rest = after;
                continue;
            };
            let after = after.trim_start();
            return Some(if let Some(quoted) = after.strip_prefix('"') {
                quoted.split('"').next().unwrap_or("").to_string()
            } else {
                after.split([',', ' ']).next().unwrap_or("").to_string()
            });
        }
    };
    (
        value_of("resource_metadata").filter(|v| !v.is_empty()),
        value_of("scope").filter(|v| !v.is_empty()),
    )
}

/// Origine e percorso di un indirizzo (`https://host:porta`, `/percorso`).
fn split_url(url: &str) -> Result<(String, String)> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| Error::msg(format!("Indirizzo non valido: {url}")))?;
    let origin = parsed.origin().ascii_serialization();
    let path = parsed.path().trim_end_matches('/').to_string();
    Ok((origin, path))
}

/// Dove cercare i metadati della risorsa protetta (RFC 9728), se il server non
/// lo dice nella risposta 401.
pub fn resource_metadata_candidates(server_url: &str) -> Result<Vec<String>> {
    let (origin, path) = split_url(server_url)?;
    let mut out = Vec::new();
    if !path.is_empty() {
        out.push(format!(
            "{origin}/.well-known/oauth-protected-resource{path}"
        ));
    }
    out.push(format!("{origin}/.well-known/oauth-protected-resource"));
    Ok(out)
}

/// Dove cercare i metadati di un server di autorizzazione (RFC 8414, poi
/// OpenID Connect), con il percorso dell'emittente inserito come chiede la RFC.
pub fn authorization_metadata_candidates(issuer: &str) -> Result<Vec<String>> {
    let (origin, path) = split_url(issuer)?;
    Ok(if path.is_empty() {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ]
    } else {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server{path}"),
            format!("{origin}/.well-known/openid-configuration{path}"),
            format!("{origin}{path}/.well-known/openid-configuration"),
        ]
    })
}

fn unsafe_address(url: &str) -> Error {
    Error::msg(format!(
        "Il server di autorizzazione indica un indirizzo non sicuro (serve https://): {url}"
    ))
}

/// Un indirizzo scoperto: deve essere sicuro come quelli dei server.
fn checked(url: &str) -> Result<String> {
    check_url(url).map_err(|_| unsafe_address(url))?;
    Ok(url.to_string())
}

async fn fetch_json(http: &reqwest::Client, url: &str) -> Option<Value> {
    if check_url(url).is_err() {
        return None;
    }
    let response = http
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

fn not_protected() -> Error {
    Error::msg(
        "Il server non chiede un accesso OAuth: scegli «Intestazioni» (o nessuna) nel modulo del server.",
    )
}

/// Scopre dove fare l'accesso per un server MCP.
pub async fn discover(http: &reqwest::Client, server_url: &str) -> Result<Endpoints> {
    check_url(server_url)?;
    // Una richiesta senza token: un server protetto risponde 401 e dice dove guardare.
    let probe = http
        .post(server_url)
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .timeout(HTTP_TIMEOUT)
        .json(&json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "personaily-hub", "version": crate::version() }
            }
        }))
        .send()
        .await
        .map_err(|e| Error::msg(format!("Server MCP non raggiungibile ({e}).")))?;
    let unauthorized = probe.status() == reqwest::StatusCode::UNAUTHORIZED;
    let (metadata_url, mut scope) = probe
        .headers()
        .get_all(reqwest::header::WWW_AUTHENTICATE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(parse_www_authenticate)
        .fold((None, None), |acc, (m, s)| (acc.0.or(m), acc.1.or(s)));

    let mut candidates = Vec::new();
    candidates.extend(metadata_url);
    candidates.extend(resource_metadata_candidates(server_url)?);
    let mut resource_meta = None;
    for url in &candidates {
        if let Some(meta) = fetch_json(http, url).await
            && meta.get("authorization_servers").is_some()
        {
            resource_meta = Some(meta);
            break;
        }
    }

    let resource = resource_meta
        .as_ref()
        .and_then(|m| m.get("resource"))
        .and_then(Value::as_str)
        .unwrap_or(server_url)
        .to_string();
    if scope.is_none() {
        scope = resource_meta
            .as_ref()
            .and_then(|m| m.get("scopes_supported"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|s| !s.is_empty());
    }
    // Senza metadati della risorsa (specifica del 2025-03-26) il server di
    // autorizzazione è il server stesso.
    let issuer = match &resource_meta {
        Some(meta) => meta
            .get("authorization_servers")
            .and_then(Value::as_array)
            .and_then(|list| list.iter().find_map(Value::as_str))
            .ok_or_else(|| Error::msg("Il server non indica il suo server di autorizzazione."))?
            .to_string(),
        None => split_url(server_url)?.0,
    };
    checked(&issuer)?;

    let mut auth_meta = None;
    for url in authorization_metadata_candidates(&issuer)? {
        if let Some(meta) = fetch_json(http, &url).await
            && meta.get("authorization_endpoint").is_some()
        {
            auth_meta = Some(meta);
            break;
        }
    }
    let Some(meta) = auth_meta else {
        if resource_meta.is_none() && !unauthorized {
            return Err(not_protected());
        }
        if resource_meta.is_some() {
            return Err(Error::msg(
                "Il server di autorizzazione non pubblica i suoi metadati: l'accesso non si può fare.",
            ));
        }
        // Le strade predefinite della specifica del 2025-03-26.
        let (origin, _) = split_url(server_url)?;
        return Ok(Endpoints {
            authorization: format!("{origin}/authorize"),
            token: format!("{origin}/token"),
            registration: Some(format!("{origin}/register")),
            resource,
            scope,
        });
    };

    if let Some(methods) = meta
        .get("code_challenge_methods_supported")
        .and_then(Value::as_array)
        && !methods.iter().any(|m| m.as_str() == Some("S256"))
    {
        return Err(Error::msg(
            "Il server di autorizzazione non supporta PKCE con S256: l'accesso sarebbe insicuro.",
        ));
    }
    let field = |key: &str| meta.get(key).and_then(Value::as_str).map(str::to_string);
    let authorization = field("authorization_endpoint").ok_or_else(|| {
        Error::msg("Il server di autorizzazione non indica la pagina di accesso.")
    })?;
    let token = field("token_endpoint").ok_or_else(|| {
        Error::msg("Il server di autorizzazione non indica dove chiedere i token.")
    })?;
    Ok(Endpoints {
        authorization: checked(&authorization)?,
        token: checked(&token)?,
        registration: field("registration_endpoint")
            .map(|url| checked(&url))
            .transpose()?,
        resource,
        scope,
    })
}

/// Registra l'app come client pubblico (RFC 7591).
async fn register(
    http: &reqwest::Client,
    endpoint: &str,
    redirect_uri: &str,
) -> Result<(String, Option<String>)> {
    let response = http
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(HTTP_TIMEOUT)
        .json(&json!({
            "client_name": "PersonAIly Hub",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none"
        }))
        .send()
        .await
        .map_err(|e| {
            Error::msg(format!(
                "Registrazione presso il servizio non riuscita ({e})."
            ))
        })?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    match body.get("client_id").and_then(Value::as_str) {
        Some(id) if status.is_success() => Ok((
            id.to_string(),
            body.get("client_secret")
                .and_then(Value::as_str)
                .map(str::to_string),
        )),
        _ => Err(Error::msg(format!(
            "Il servizio ha rifiutato la registrazione dell'app ({status}). {}",
            oauth_error(&body)
        ))),
    }
}

/// Il messaggio d'errore OAuth di una risposta, se c'è.
fn oauth_error(body: &Value) -> String {
    body.get("error_description")
        .or_else(|| body.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Prepara l'accesso: scoperta, registrazione, porta di ritorno, PKCE.
/// `config` ha già il segreto del client preso dal portachiavi.
pub async fn begin(
    http: &reqwest::Client,
    config: &McpServerConfig,
) -> Result<(PendingLogin, LoginStart)> {
    let endpoints = discover(http, config.url.trim()).await?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let redirect_uri = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );

    let given_id = config.client_id.trim();
    let (client_id, client_secret, registered) = if !given_id.is_empty() {
        let secret = Some(config.client_secret.clone()).filter(|s| !s.is_empty());
        (given_id.to_string(), secret, false)
    } else {
        let endpoint = endpoints.registration.as_deref().ok_or_else(|| {
            Error::msg(
                "Il servizio non permette all'app di registrarsi da sola: registra un'app OAuth presso il servizio (indirizzo di ritorno http://127.0.0.1/callback) e scrivi il suo Client ID nel modulo.",
            )
        })?;
        let (id, secret) = register(http, endpoint, &redirect_uri).await?;
        (id, secret, true)
    };

    let verifier = random_token(32)?;
    let state = random_token(16)?;
    let mut url = reqwest::Url::parse(&endpoints.authorization)
        .map_err(|_| unsafe_address(&endpoints.authorization))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("code_challenge", &challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", &endpoints.resource);
        if let Some(scope) = &endpoints.scope {
            query.append_pair("scope", scope);
        }
    }
    let host = url.host_str().unwrap_or("").to_string();
    Ok((
        PendingLogin {
            listener,
            state,
            verifier,
            redirect_uri,
            token_endpoint: endpoints.token,
            client_id,
            client_secret,
            registered,
            resource: endpoints.resource,
        },
        LoginStart {
            url: url.to_string(),
            host,
        },
    ))
}

const DONE_PAGE: &str = "<!doctype html><meta charset=\"utf-8\"><title>PersonAIly Hub</title>\
<body style=\"font-family:sans-serif;margin:3em\"><h1>PersonAIly Hub</h1>\
<p>Accesso completato: puoi chiudere questa scheda e tornare all'app.</p>\
<p lang=\"en\">Signed in: you can close this tab and go back to the app.</p></body>";
const FAILED_PAGE: &str = "<!doctype html><meta charset=\"utf-8\"><title>PersonAIly Hub</title>\
<body style=\"font-family:sans-serif;margin:3em\"><h1>PersonAIly Hub</h1>\
<p>Accesso non riuscito: torna all'app per i dettagli.</p>\
<p lang=\"en\">Sign-in failed: go back to the app for details.</p></body>";

async fn reply(stream: &mut tokio::net::TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Aspetta che il browser torni su `/callback` e restituisce il codice.
/// Altre richieste (per esempio l'icona della pagina) ricevono un 404.
pub async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        // Basta la prima riga della richiesta; si legge al massimo 16 kB.
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await {
                Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                _ => break,
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let target = text
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
            reply(&mut stream, "400 Bad Request", "").await;
            continue;
        };
        if url.path() != "/callback" {
            reply(&mut stream, "404 Not Found", "").await;
            continue;
        }
        let get = |key: &str| {
            url.query_pairs()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned())
        };
        // Una richiesta con un altro `state` non viene dall'accesso avviato:
        // un'altra scheda rimasta aperta, o un programma qualsiasi di questo
        // computer. Si rifiuta e si continua ad aspettare quella giusta, così
        // nessuno può far fallire l'accesso solo aprendo questo indirizzo.
        if get("state").as_deref() != Some(state) {
            reply(&mut stream, "400 Bad Request", FAILED_PAGE).await;
            continue;
        }
        if let Some(error) = get("error") {
            reply(&mut stream, "200 OK", FAILED_PAGE).await;
            let detail = get("error_description").unwrap_or(error);
            return Err(Error::msg(format!(
                "Accesso non concesso dal servizio: {detail}"
            )));
        }
        match get("code").filter(|c| !c.is_empty()) {
            Some(code) => {
                reply(&mut stream, "200 OK", DONE_PAGE).await;
                return Ok(code);
            }
            None => {
                reply(&mut stream, "400 Bad Request", FAILED_PAGE).await;
                return Err(Error::msg(
                    "Il servizio non ha restituito il codice di accesso.",
                ));
            }
        }
    }
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("http://x/").expect("indirizzo fisso");
    url.query_pairs_mut().extend_pairs(pairs);
    url.query().unwrap_or("").to_string()
}

/// Chiede i token all'indirizzo dei token.
async fn token_request(
    http: &reqwest::Client,
    endpoint: &str,
    pairs: &[(&str, &str)],
) -> Result<TokenSet> {
    let response = http
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .timeout(HTTP_TIMEOUT)
        .body(form(pairs))
        .send()
        .await
        .map_err(|e| Error::msg(format!("Richiesta dei token non riuscita ({e}).")))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    // Qualche servizio risponde in formato modulo anche se si chiede JSON.
    let body: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
        let parsed = reqwest::Url::parse(&format!("http://x/?{}", text.trim()));
        Value::Object(
            parsed
                .map(|u| {
                    u.query_pairs()
                        .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                        .collect()
                })
                .unwrap_or_default(),
        )
    });
    let access = body.get("access_token").and_then(Value::as_str);
    match access {
        Some(access) if status.is_success() && !access.is_empty() => Ok(TokenSet {
            access: access.to_string(),
            refresh: body
                .get("refresh_token")
                .and_then(Value::as_str)
                .filter(|r| !r.is_empty())
                .map(str::to_string),
            expires_in: body.get("expires_in").and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            }),
        }),
        _ => Err(Error::msg(format!(
            "Il servizio non ha dato i token ({status}). {}",
            oauth_error(&body)
        ))),
    }
}

/// Scambia il codice con i token.
pub async fn exchange(
    http: &reqwest::Client,
    login: &PendingLogin,
    code: &str,
) -> Result<TokenSet> {
    let mut pairs = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", login.redirect_uri.as_str()),
        ("client_id", login.client_id.as_str()),
        ("code_verifier", login.verifier.as_str()),
        ("resource", login.resource.as_str()),
    ];
    if let Some(secret) = &login.client_secret {
        pairs.push(("client_secret", secret));
    }
    token_request(http, &login.token_endpoint, &pairs).await
}

/// Rinnova il token di accesso.
pub async fn refresh(
    http: &reqwest::Client,
    record: &OAuthRecord,
    refresh_token: &str,
    client_secret: Option<&str>,
) -> Result<TokenSet> {
    let mut pairs = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", record.client_id.as_str()),
        ("resource", record.resource.as_str()),
    ];
    if let Some(secret) = client_secret {
        pairs.push(("client_secret", secret));
    }
    token_request(http, &record.token_endpoint, &pairs).await
}

/// Salva un valore nel portachiavi, diviso in pezzi se è lungo; con `None`
/// lo toglie.
pub fn put_secret(vault: &dyn EnvVault, id: &str, name: &str, value: Option<&str>) -> Result<()> {
    let chars: Vec<char> = value.unwrap_or("").chars().collect();
    let pieces: Vec<String> = chars
        .chunks(CHUNK_CHARS)
        .map(|c| c.iter().collect())
        .collect();
    for (n, piece) in pieces.iter().enumerate() {
        vault.set(id, &format!("{name}#{n}"), piece)?;
    }
    for n in pieces.len()..MAX_CHUNKS {
        vault.delete(id, &format!("{name}#{n}"))?;
    }
    Ok(())
}

/// Rilegge un valore salvato con [`put_secret`].
pub fn get_secret(vault: &dyn EnvVault, id: &str, name: &str) -> Result<Option<String>> {
    let mut out = String::new();
    for n in 0..MAX_CHUNKS {
        match vault.get(id, &format!("{name}#{n}"))? {
            Some(piece) => out.push_str(&piece),
            None => break,
        }
    }
    Ok(Some(out).filter(|s| !s.is_empty()))
}

/// Mette il token di accesso nelle intestazioni della configurazione, al posto
/// di un'eventuale `Authorization` scritta a mano.
pub fn apply_token(config: &mut McpServerConfig, access: &str) {
    config
        .headers
        .retain(|(name, _)| !name.trim().eq_ignore_ascii_case("authorization"));
    config
        .headers
        .push(("Authorization".into(), format!("Bearer {access}")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legge_www_authenticate() {
        let (meta, scope) = parse_www_authenticate(
            r#"Bearer error="invalid_token", resource_metadata="https://x.it/.well-known/oauth-protected-resource/mcp", scope="read write""#,
        );
        assert_eq!(
            meta.as_deref(),
            Some("https://x.it/.well-known/oauth-protected-resource/mcp")
        );
        assert_eq!(scope.as_deref(), Some("read write"));
        let (meta, scope) =
            parse_www_authenticate("Bearer resource_metadata=https://y.it/m, xscope=no");
        assert_eq!(meta.as_deref(), Some("https://y.it/m"));
        assert_eq!(scope, None);
        assert_eq!(parse_www_authenticate("Basic realm=\"x\""), (None, None));
    }

    #[test]
    fn gli_indirizzi_dei_metadati() {
        assert_eq!(
            resource_metadata_candidates("https://mcp.esempio.it/v1/mcp/").unwrap(),
            [
                "https://mcp.esempio.it/.well-known/oauth-protected-resource/v1/mcp",
                "https://mcp.esempio.it/.well-known/oauth-protected-resource",
            ]
        );
        assert_eq!(
            authorization_metadata_candidates("https://auth.esempio.it").unwrap(),
            [
                "https://auth.esempio.it/.well-known/oauth-authorization-server",
                "https://auth.esempio.it/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            authorization_metadata_candidates("https://esempio.it/tenant/1").unwrap()[0],
            "https://esempio.it/.well-known/oauth-authorization-server/tenant/1"
        );
    }

    #[test]
    fn pkce_s256() {
        // Valore calcolato a parte con Python: base64url(sha256(verificatore)), senza "=".
        assert_eq!(
            challenge("dBjftJeZ4CVP-mJ92K1aB2MDjHGsKMLOxv5rTtBBdbw"),
            "UhTAGYTK3knWlcORYNArHPCqIAKu6hegDu369tPqjVU"
        );
        let a = random_token(32).unwrap();
        assert_eq!(a.len(), 43);
        assert_ne!(a, random_token(32).unwrap());
    }

    #[test]
    fn i_token_lunghi_si_dividono_nel_portachiavi() {
        let vault = crate::mcp::tests::MemoryVault::default();
        let long = "x".repeat(2500);
        put_secret(&vault, "s", ACCESS_KEY, Some(&long)).unwrap();
        assert_eq!(vault.0.lock().unwrap().len(), 3);
        assert_eq!(
            get_secret(&vault, "s", ACCESS_KEY).unwrap().as_deref(),
            Some(long.as_str())
        );
        put_secret(&vault, "s", ACCESS_KEY, Some("corto")).unwrap();
        assert_eq!(vault.0.lock().unwrap().len(), 1);
        put_secret(&vault, "s", ACCESS_KEY, None).unwrap();
        assert_eq!(get_secret(&vault, "s", ACCESS_KEY).unwrap(), None);
    }

    #[test]
    fn il_token_sostituisce_l_intestazione_scritta_a_mano() {
        let mut config = McpServerConfig {
            headers: vec![
                ("authorization".into(), "Bearer vecchio".into()),
                ("X-A".into(), "1".into()),
            ],
            ..Default::default()
        };
        apply_token(&mut config, "nuovo");
        assert_eq!(
            config.headers,
            [
                ("X-A".to_string(), "1".to_string()),
                ("Authorization".to_string(), "Bearer nuovo".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn un_ritorno_con_state_diverso_si_rifiuta() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            let http = reqwest::Client::new();
            let _ = http
                .get(format!("http://127.0.0.1:{port}/favicon.ico"))
                .send()
                .await;
            // Un ritorno estraneo, anche con un errore: rifiutato, senza
            // interrompere l'accesso.
            let foreign = http
                .get(format!(
                    "http://127.0.0.1:{port}/callback?error=access_denied&state=altro"
                ))
                .send()
                .await
                .unwrap()
                .status();
            let right = http
                .get(format!(
                    "http://127.0.0.1:{port}/callback?code=buono&state=giusto"
                ))
                .send()
                .await
                .unwrap()
                .status();
            (foreign, right)
        });
        let result = wait_for_code(&listener, "giusto").await;
        assert_eq!(result.unwrap(), "buono");
        let (foreign, right) = browser.await.unwrap();
        assert_eq!(foreign, 400);
        assert_eq!(right, 200);
    }

    /// Scoperta contro servizi veri: solo letture di metadati pubblici, nessuna
    /// registrazione. Da lanciare a mano: `cargo test -p app-core scoperta_dal_vivo -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "usa internet"]
    async fn scoperta_dal_vivo() {
        let http = reqwest::Client::new();
        for url in ["https://mcp.notion.com/mcp", "https://mcp.linear.app/mcp"] {
            match discover(&http, url).await {
                Ok(found) => println!("{url}\n  {found:?}"),
                Err(e) => println!("{url}\n  ERRORE {e}"),
            }
        }
    }
}
