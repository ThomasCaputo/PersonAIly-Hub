# @personaily/api-client

Tipi TypeScript dell'API locale di PersonAIly Hub (`/api/...`), generati dai tipi
Rust del nucleo con [ts-rs](https://github.com/Aleph-Alpha/ts-rs): così l'estensione
VS Code (e qualunque altro client) non tiene una copia scritta a mano che può
restare indietro.

`index.d.ts` non si modifica a mano. Dopo aver cambiato un tipo dell'API:

```
UPDATE_BINDINGS=1 cargo test -p app-server bindings
```

`cargo test` (anche in CI) fallisce se il file non corrisponde più ai tipi Rust.
L'elenco dei tipi esportati sta in `crates/server/src/bindings.rs`.

Solo tipi: l'estensione li importa con `import type`, quindi nel pacchetto
`.vsix` non finisce nulla di questa cartella.
