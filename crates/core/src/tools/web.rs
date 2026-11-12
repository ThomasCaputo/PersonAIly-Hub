//! Ricerca web e lettura di pagine.
//!
//! La ricerca usa la versione HTML di DuckDuckGo, che non richiede chiave né
//! account. Le pagine si scaricano solo da indirizzi pubblici: un modello non
//! deve poter usare questo strumento per raggiungere il computer dell'utente
//! o la sua rete di casa.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Url;

use crate::error::{Error, Result};
use crate::tools::files::truncate;

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
const MAX_RESULTS: usize = 8;
const MAX_REDIRECTS: usize = 5;
/// Quanto di una pagina si scarica al massimo.
const MAX_PAGE_BYTES: usize = 3 * 1024 * 1024;
/// Quanti caratteri di una pagina tornano al modello.
const MAX_PAGE_CHARS: usize = 30_000;
const TIMEOUT: Duration = Duration::from_secs(25);

/// Client per il web: i redirect si seguono a mano, per controllare ogni tappa.
pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        // I siti rispondono in modo diverso, o non rispondono, ai client senza nome.
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36")
        .build()
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Cerca sul web e restituisce i risultati come testo per il modello.
pub async fn search(http: &reqwest::Client, query: &str) -> Result<String> {
    let query = query.trim();
    if query.is_empty() {
        return Err(Error::msg("La ricerca è vuota."));
    }
    // Le parole cercate viaggiano nel corpo della richiesta, codificate come in un modulo web.
    let form = Url::parse_with_params(SEARCH_URL, [("q", query)])
        .ok()
        .and_then(|url| url.query().map(str::to_string))
        .unwrap_or_default();
    let response = http
        .post(SEARCH_URL)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "Il motore di ricerca ha risposto con un errore ({}).",
            response.status()
        )));
    }
    let results = parse_results(&response.text().await?);
    if results.is_empty() {
        return Ok(format!("Nessun risultato per \"{query}\"."));
    }
    let mut out = format!("Risultati per \"{query}\":\n");
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {}\n   {}\n   {}\n",
            i + 1,
            r.title,
            r.url,
            r.snippet
        ));
    }
    Ok(out)
}

/// Estrae i risultati dalla pagina HTML di DuckDuckGo.
fn parse_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    // Ogni risultato comincia con il collegamento del titolo.
    for chunk in html.split("class=\"result__a\"").skip(1) {
        let Some(href) = between(chunk, "href=\"", "\"") else {
            continue;
        };
        let Some(url) = real_url(&decode_entities(href)) else {
            continue;
        };
        let title = between(chunk, ">", "</a>")
            .map(html_to_text)
            .unwrap_or_default();
        let snippet = chunk
            .split_once("class=\"result__snippet\"")
            .and_then(|(_, rest)| between(rest, ">", "</a>"))
            .map(html_to_text)
            .unwrap_or_default();
        if !title.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }
        if results.len() == MAX_RESULTS {
            break;
        }
    }
    results
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let len = text[from..].find(end)?;
    Some(&text[from..from + len])
}

/// DuckDuckGo non collega direttamente i siti ma un proprio indirizzo di
/// passaggio, con la destinazione nel parametro `uddg`. La pubblicità si scarta.
fn real_url(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let url = Url::parse(&absolute).ok()?;
    let is_ddg = url
        .host_str()
        .is_some_and(|h| h.ends_with("duckduckgo.com"));
    if !is_ddg {
        return Some(absolute);
    }
    if url.path().starts_with("/y.js") {
        return None;
    }
    url.query_pairs()
        .find(|(key, _)| key == "uddg")
        .map(|(_, value)| value.into_owned())
}

/// Scarica una pagina pubblica e ne restituisce il testo.
pub async fn read_page(http: &reqwest::Client, address: &str) -> Result<String> {
    let mut url = Url::parse(address.trim())
        .map_err(|_| Error::msg("Indirizzo non valido: deve essere completo, con https://."))?;

    for _ in 0..=MAX_REDIRECTS {
        check_public(&url).await?;
        let mut response = http.get(url.clone()).send().await?;
        let status = response.status();

        if status.is_redirection() {
            let target = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    Error::msg("Il sito ha risposto con un rinvio senza destinazione.")
                })?;
            url = url
                .join(target)
                .map_err(|_| Error::msg("Il sito ha risposto con un rinvio non valido."))?;
            continue;
        }
        if !status.is_success() {
            return Err(Error::msg(format!(
                "Il sito ha risposto con un errore ({status})."
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_ascii_lowercase();
        let readable = ["text/", "json", "xml"]
            .iter()
            .any(|kind| content_type.contains(kind));
        if !readable {
            return Err(Error::msg(format!(
                "La pagina non è testo ({content_type}): non posso leggerla."
            )));
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_PAGE_BYTES {
                bytes.truncate(MAX_PAGE_BYTES);
                break;
            }
        }
        let body = String::from_utf8_lossy(&bytes);
        let text = if content_type.contains("html") {
            html_to_text(&body)
        } else {
            body.into_owned()
        };
        if text.trim().is_empty() {
            return Ok(format!(
                "La pagina {url} non contiene testo leggibile (forse è costruita con JavaScript)."
            ));
        }
        return Ok(format!(
            "Testo di {url}:\n\n{}",
            truncate(text, MAX_PAGE_CHARS)
        ));
    }
    Err(Error::msg("Troppi rinvii: il sito non porta a una pagina."))
}

/// Rifiuta gli indirizzi che non sono su internet: questo computer, la rete
/// locale, gli indirizzi riservati. Vale per ogni tappa di un rinvio.
async fn check_public(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::msg(
            "Si possono leggere solo pagine http:// o https://.",
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::msg("Indirizzo senza nome del sito."))?;
    let port = url.port_or_known_default().unwrap_or(443);
    // Per un IPv6 scritto tra parentesi quadre il nome va passato senza.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<IpAddr> = tokio::net::lookup_host((bare, port))
        .await
        .map_err(|_| Error::msg(format!("Sito non trovato: {host}")))?
        .map(|a| a.ip())
        .collect();
    if addresses.is_empty() || !addresses.iter().all(is_public) {
        return Err(Error::msg(format!(
            "{host} non è un sito pubblico: questo strumento non raggiunge il computer \
             dell'utente né la sua rete locale."
        )));
    }
    Ok(())
}

fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                // 100.64.0.0/10: indirizzi condivisi dagli operatori.
                || (o[0] == 100 && (64..128).contains(&o[1]))
                // 192.0.0.0/24 e 198.18.0.0/15: riservati.
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (18..20).contains(&o[1]))
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(&IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 locali, fe80::/10 di collegamento.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// Elementi il cui contenuto non è testo da leggere.
const SKIPPED: &[&str] = &["script", "style", "noscript", "svg", "template", "head"];
/// Elementi che vanno a capo.
const BLOCKS: &[&str] = &[
    "p",
    "br",
    "div",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "header",
    "footer",
    "blockquote",
    "pre",
    "table",
    "ul",
    "ol",
    "title",
];

/// Toglie i tag da una pagina HTML e lascia il testo, con gli a capo essenziali.
pub fn html_to_text(html: &str) -> String {
    // La copia in minuscolo ha la stessa lunghezza in byte: serve a cercare i tag.
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 4);
    let mut pos = 0;

    while let Some(open) = html[pos..].find('<').map(|i| pos + i) {
        out.push_str(&html[pos..open]);

        // Un `<` che non apre un tag è un segno di minore nel testo.
        let starts_tag = html[open + 1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!');
        if !starts_tag {
            out.push('<');
            pos = open + 1;
            continue;
        }
        if lower[open..].starts_with("<!--") {
            pos = lower[open..]
                .find("-->")
                .map_or(html.len(), |i| open + i + 3);
            continue;
        }
        let Some(close) = html[open..].find('>').map(|i| open + i) else {
            pos = html.len();
            break;
        };
        let tag = lower[open + 1..close].trim_start_matches('/');
        let name: &str = tag
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or("");
        let closing = lower[open + 1..].starts_with('/');
        pos = close + 1;

        if !closing && SKIPPED.contains(&name) {
            let end_tag = format!("</{name}");
            pos = match lower[pos..].find(&end_tag) {
                Some(i) => {
                    let end = pos + i;
                    html[end..].find('>').map_or(html.len(), |j| end + j + 1)
                }
                None => html.len(),
            };
        } else if BLOCKS.contains(&name) {
            out.push('\n');
        }
    }
    out.push_str(&html[pos..]);
    tidy(&decode_entities(&out))
}

/// Riduce gli spazi ripetuti e toglie le righe vuote: al modello servono le
/// parole, non l'impaginazione.
fn tidy(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // Un'entità è breve e finisce con `;`.
        let decoded = rest[1..]
            .char_indices()
            .take(10)
            .find(|(_, c)| *c == ';')
            .and_then(|(end, _)| Some((entity(&rest[1..1 + end])?, end + 2)));
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "egrave" => 'è',
        "eacute" => 'é',
        "agrave" => 'à',
        "igrave" => 'ì',
        "ograve" => 'ò',
        "ugrave" => 'ù',
        "euro" => '€',
        "middot" => '·',
        "bull" => '•',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "times" => '×',
        "shy" => '\u{ad}',
        "ccedil" => 'ç',
        "Egrave" => 'È',
        "Agrave" => 'À',
        "laquo" => '«',
        "raquo" => '»',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "rsquo" => '’',
        "lsquo" => '‘',
        "rdquo" => '”',
        "ldquo" => '“',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toglie_i_tag_e_lascia_il_testo() {
        let html = "<html><head><title>Titolo</title><style>p{color:red}</style></head>\
                    <body><!-- nota --><h1>Ciao &amp; benvenuti</h1>\
                    <SCRIPT>alert('<p>no</p>')</SCRIPT>\
                    <p>Prima   riga<br>seconda &egrave; qui&#33; &#x41;</p>\
                    <ul><li>uno</li><li>due</li></ul> 5 &lt; 6 &sconosciuta; a & b</body></html>";
        assert_eq!(
            html_to_text(html),
            "Ciao & benvenuti\nPrima riga\nseconda è qui! A\nuno\ndue\n5 < 6 &sconosciuta; a & b"
        );
        assert_eq!(html_to_text("se a < b e b > c"), "se a < b e b > c");
        assert_eq!(html_to_text("solo testo"), "solo testo");
        assert_eq!(html_to_text("tag <b>non chiuso"), "tag non chiuso");
        assert_eq!(html_to_text("<script>mai chiuso"), "");
    }

    #[test]
    fn legge_i_risultati_di_ricerca() {
        let html = r#"
          <div class="result"><h2 class="result__title">
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust%2Dlang.org%2Fit%2F&amp;rut=abc">Il linguaggio <b>Rust</b></a></h2>
            <a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">Un linguaggio <b>affidabile</b> ed efficiente.</a></div>
          <div class="result result--ad"><a class="result__a" href="https://duckduckgo.com/y.js?ad_domain=x">Pubblicità</a></div>
          <div class="result"><a class="result__a" href="https://example.org/diretto">Collegamento diretto</a></div>"#;
        let results = parse_results(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Il linguaggio Rust");
        assert_eq!(results[0].url, "https://www.rust-lang.org/it/");
        assert_eq!(
            results[0].snippet,
            "Un linguaggio affidabile ed efficiente."
        );
        assert_eq!(results[1].url, "https://example.org/diretto");
        assert_eq!(results[1].snippet, "");
    }

    #[test]
    fn riconosce_gli_indirizzi_non_pubblici() {
        let public = |text: &str| is_public(&text.parse().unwrap());
        assert!(public("93.184.216.34"));
        assert!(public("2606:2800:220:1:248:1893:25c8:1946"));
        for private in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!public(private), "{private} dovrebbe essere rifiutato");
        }
    }

    #[tokio::test]
    async fn non_legge_questo_computer_ne_file_locali() {
        let http = client().unwrap();
        assert!(
            read_page(&http, "http://127.0.0.1:8080/segreto")
                .await
                .is_err()
        );
        assert!(read_page(&http, "http://localhost/").await.is_err());
        assert!(
            read_page(&http, "file:///C:/Windows/win.ini")
                .await
                .is_err()
        );
        assert!(read_page(&http, "non un indirizzo").await.is_err());
    }
}
