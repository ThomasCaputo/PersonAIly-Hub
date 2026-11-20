# 0018 — Agenti che delegano ad altri agenti (AG-8)

**Stato:** accettata e realizzata (20-11-2026). Provata nell'app con Qwen2.5 7B: un
"Coordinatore" senza strumenti sui file affida a uno "Scrittore" la creazione di un file
e ne riassume il resoconto.

## Decisione

- Ogni agente indica i suoi **aiutanti** (altri agenti). Nelle sue sessioni il modello
  riceve lo strumento `delegate_task(agent, task)`, con l'elenco degli aiutanti e delle
  loro descrizioni.
- Chiamarlo apre una **sessione figlia** dell'aiutante (`parent_session_id`), con le sue
  cartelle, i suoi strumenti e i suoi permessi; la si segue fino alla fine e la sua ultima
  risposta torna come risultato dello strumento. La sessione figlia si vede nell'elenco
  ("↳ Coordinatore") e le sue conferme si danno lì.
- Lo strumento è in classe "esecuzione": chiede conferma salvo con "Consenti tutto".
- **Limiti**: al massimo due livelli di deleghe; un agente non delega a sé stesso né a chi
  ha già lavoro nella catena. Fermare il genitore ferma il figlio.
- Con il modello locale le richieste si alternano: mentre il figlio lavora, il genitore
  aspetta il risultato dello strumento e non occupa `llama-server`.

## Conseguenze

- Il compito va scritto per intero: l'aiutante non vede la conversazione del genitore.
- Il costo dei modelli cloud si somma tra le sessioni; il limite di spesa vale per ognuna.
