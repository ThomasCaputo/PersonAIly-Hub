import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Conversation } from "../api";
import { t } from "../i18n";

/** Un'azione della tavolozza. */
export type PaletteCommand = {
  id: string;
  label: string;
  /** Gruppo mostrato a destra: "Vai a", "Conversazione"… */
  group: string;
  run: () => void;
};

type Props = {
  commands: PaletteCommand[];
  onOpenConversation: (id: number) => void;
  onClose: () => void;
};

/** Quante conversazioni mostrare tra i risultati. */
const MAX_CONVERSATIONS = 8;

/** Tutte le parole del filtro compaiono nel testo, in qualunque ordine. */
function matches(text: string, filter: string): boolean {
  const haystack = text.toLocaleLowerCase();
  return filter
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => haystack.includes(word));
}

/**
 * Tavolozza dei comandi (Ctrl+K): si scrive, si sceglie con le frecce e si
 * conferma con Invio. Oltre ai comandi cerca nelle conversazioni, anche nel
 * testo dei messaggi (la stessa ricerca della barra laterale della chat).
 */
export function CommandPalette({ commands, onOpenConversation, onClose }: Props) {
  const [filter, setFilter] = useState("");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => inputRef.current?.focus(), []);

  // La ricerca nelle conversazioni aspetta che si smetta di scrivere.
  useEffect(() => {
    let current = true;
    const timer = window.setTimeout(() => {
      api
        .listConversations(filter.trim())
        .then((list) => current && setConversations(list.slice(0, MAX_CONVERSATIONS)))
        .catch(() => {});
    }, 120);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [filter]);

  const items = useMemo<PaletteCommand[]>(() => {
    const own = commands.filter((c) => matches(`${c.label} ${c.group}`, filter));
    const found = conversations.map((c) => ({
      id: `conversation-${c.id}`,
      label: c.title,
      group: t("Conversazione"),
      run: () => onOpenConversation(c.id),
    }));
    return [...own, ...found];
  }, [commands, conversations, filter, onOpenConversation]);

  useEffect(() => setSelected(0), [filter]);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const choose = (item: PaletteCommand | undefined) => {
    if (!item) return;
    onClose();
    item.run();
  };

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label={t("Tavolozza dei comandi")}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <input
          ref={inputRef}
          className="palette-input"
          placeholder={t("Cerca un comando o una conversazione…")}
          value={filter}
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-activedescendant={items[selected] ? `palette-${items[selected].id}` : undefined}
          onChange={(e) => setFilter(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setSelected((s) => Math.min(items.length - 1, s + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSelected((s) => Math.max(0, s - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              choose(items[selected]);
            } else if (e.key === "Escape") {
              e.preventDefault();
              onClose();
            }
          }}
        />
        <ul id="palette-list" className="palette-list" role="listbox" ref={listRef}>
          {items.map((item, index) => (
            <li
              key={item.id}
              id={`palette-${item.id}`}
              data-index={index}
              role="option"
              aria-selected={index === selected}
              className={index === selected ? "palette-item selected" : "palette-item"}
              onMouseEnter={() => setSelected(index)}
              onClick={() => choose(item)}
            >
              <span className="palette-label">{item.label}</span>
              <span className="palette-group dim">{item.group}</span>
            </li>
          ))}
          {items.length === 0 && <li className="palette-empty dim">{t("Nessun risultato")}</li>}
        </ul>
      </div>
    </div>
  );
}
