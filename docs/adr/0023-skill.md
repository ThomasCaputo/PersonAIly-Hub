# 0023 — Skill

**Stato:** accettata e realizzata (25-11-2026). Provata nell'app con Qwen2.5 7B: con una
skill «firma-email» il modello carica le istruzioni, legge il file `firma.txt` della skill e
chiude l'email con quella firma, senza conferme.

## Contesto

Per i compiti che si ripetono (scrivere un verbale, rivedere un testo con certe regole, preparare
un rapporto) oggi bisogna riscrivere le istruzioni o metterle nel prompt di sistema, dove pesano
su ogni messaggio. Con i modelli locali dal contesto corto è uno spreco, e un prompt lungo
confonde i modelli piccoli.

Esiste un formato aperto per questo, le *Agent Skills*: una cartella con un file `SKILL.md`
(un'intestazione YAML con `name` e `description`, poi le istruzioni in Markdown) ed eventuali
file di supporto (modelli, esempi, script). È lo stesso formato usato da Claude e da altri
programmi, quindi le skill già scritte si possono riusare.

## Decisione

- **Dove stanno.** Ogni skill è una cartella in `<dati>/skills/<nome>/`. Si aggiungono da
  **Impostazioni → Skill**:
  - **Importa una cartella** o **Importa un file .zip** (lo `SKILL.md` può stare in cima allo
    zip o in una sua unica cartella);
  - **Nuova skill**: nome, descrizione e istruzioni scritti nell'app (diventano lo `SKILL.md`).
  Ogni skill si può accendere, spegnere, modificare (solo lo `SKILL.md`) ed eliminare.
- **Regole del formato.** Il nome ha solo lettere minuscole, cifre e trattini (al massimo 64
  caratteri); la descrizione è obbligatoria (al massimo 1024). Un nome già presente chiede di
  sostituire la skill. Importando si copiano al massimo 200 file e 20 MB; collegamenti e
  giunzioni si saltano; nei `.zip` si rifiutano i percorsi che escono dalla cartella.
- **Caricamento su richiesta.** Con lo strumento **Skill** acceso (nella barra della chat,
  negli strumenti predefiniti e negli agenti), il prompt di sistema riceve solo nome e
  descrizione delle skill accese. Il modello ha due strumenti:
  - `load_skill(name)`: restituisce le istruzioni, la cartella della skill e l'elenco dei suoi file;
  - `read_skill_file(name, path)`: legge un file di testo della skill (al massimo 200 kB),
    mai fuori dalla sua cartella.
  Sono strumenti di sola lettura su file che l'utente stesso ha installato: non chiedono conferma.
- **Le istruzioni di una skill valgono come istruzioni.** Il preambolo dice che ciò che
  restituiscono gli strumenti è materiale da consultare, non ordini. Le skill sono l'eccezione:
  le ha scelte l'utente. Una skill però **non concede permessi**: file, web, terminale e MCP
  restano sotto le regole di sempre (SI-5). Gli script di una skill si possono eseguire solo
  dove c'è il terminale, cioè negli agenti, e con le sue conferme.
- **Nei backup.** La cartella `skills/` entra nel `.zip` del backup, come i preset.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Mettere tutte le istruzioni nel prompt di sistema | Peso su ogni messaggio, e i modelli piccoli si confondono |
| Skill scelte una per una in ogni conversazione | Troppi clic; la descrizione basta al modello per scegliere, e l'utente le spegne in Impostazioni |
| Conferma prima di leggere una skill | È sola lettura di file installati dall'utente: una conferma in più non protegge nulla |
| Script delle skill eseguiti dall'app in automatico | Una skill scaricata potrebbe eseguire codice qualsiasi: passa dal terminale e dalle sue conferme |
| Un formato proprio | Le skill esistenti non sarebbero riusabili |

## Conseguenze

- Le skill pesano poco finché non servono: ognuna costa una riga nel prompt.
- Una skill scaricata può contenere istruzioni ingannevoli: il modello le segue come se le avesse
  scritte l'utente. I permessi restano comunque del codice, non del testo.
- Le skill che dipendono da script (Python, Node) funzionano solo negli agenti con il terminale.
