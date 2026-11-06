<script lang="ts">
  import { api, plainText, type ComicMeta, type MetaField } from "$lib/api";
  import { lib, sheets } from "$lib/state.svelte";
  import { untrack } from "svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  // Edit any book's details by hand. Your edits win over Comic Vine and the YACReader import,
  // and the undo arrow next to an edited field brings back the value underneath it.
  let { id }: { id: number } = $props();

  type Field = { key: MetaField; label: string; wide?: boolean; lines?: number; hint?: string };
  const FIELDS: Field[] = [
    { key: "title", label: "Title", wide: true },
    { key: "volume", label: "Series" },
    { key: "number", label: "Number" },
    { key: "date", label: "Cover date", hint: "YYYY-MM-DD" },
    { key: "year", label: "Year" },
    { key: "writer", label: "Writer", lines: 2 },
    { key: "penciller", label: "Artist", lines: 2 },
    { key: "inker", label: "Inker", lines: 2 },
    { key: "colorist", label: "Colorist", lines: 2 },
    { key: "letterer", label: "Letterer", lines: 2 },
    { key: "cover_artist", label: "Cover artist", lines: 2 },
    { key: "publisher", label: "Publisher" },
    { key: "genre", label: "Genre" },
    { key: "story_arc", label: "Story arc" },
    { key: "arc_number", label: "Part" },
    { key: "arc_count", label: "Of" },
    { key: "characters", label: "Characters", lines: 4, hint: "one per line" },
    { key: "teams", label: "Teams", lines: 4, hint: "one per line" },
    { key: "locations", label: "Locations", lines: 4, hint: "one per line", wide: true },
    { key: "synopsis", label: "Synopsis", wide: true, lines: 6 },
    { key: "cv_id", label: "Comic Vine ID" },
  ];

  let meta = $state<ComicMeta | null>(null);
  let values = $state<Record<string, string>>({});
  /** Fields whose edit should be dropped on save. */
  let reverting = $state<Set<string>>(new Set());
  let error = $state("");
  let saving = $state(false);

  // Synopses arrive as HTML from Comic Vine and YACReader; edit them as plain text.
  const shown = (k: MetaField, v: string | null) => (k === "synopsis" ? plainText(v) : (v ?? ""));

  function load(m: ComicMeta) {
    meta = m;
    values = Object.fromEntries(FIELDS.map((f) => [f.key, shown(f.key, m.fields[f.key])]));
    reverting = new Set();
  }

  // The sheet is recreated for each book, so loading once is enough.
  api.comicMeta(untrack(() => id)).then(load).catch((e) => (error = String(e)));

  const changed = $derived.by(() => {
    if (!meta) return {};
    const out: Partial<Record<MetaField, string | null>> = {};
    for (const f of FIELDS) {
      if (reverting.has(f.key)) out[f.key] = null;
      else if (values[f.key].trim() !== shown(f.key, meta.fields[f.key]).trim()) out[f.key] = values[f.key].trim();
    }
    return out;
  });
  const dirty = $derived(Object.keys(changed).length > 0);
  const edited = (k: MetaField) => !!meta?.edited.includes(k) && !reverting.has(k);
  const fromCv = (k: MetaField) => !edited(k) && !!meta?.fromCv.includes(k);

  function revert(k: MetaField) {
    reverting = new Set([...reverting, k]);
  }

  async function save() {
    if (!dirty) return close();
    saving = true;
    error = "";
    try {
      load(await api.editComicMeta(id, changed));
      lib.version++;
      close();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function revertAll() {
    if (!meta) return;
    reverting = new Set(meta.edited);
  }

  const close = () => (sheets.editing = null);

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
    else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) save();
    else return;
    e.preventDefault();
    e.stopImmediatePropagation();
  }
</script>

<svelte:window onkeydowncapture={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="sheet" role="dialog" aria-label="Edit details">
    {#if meta}
      <header>
        <button class="ico x" title="Close (Esc)" aria-label="Close" onclick={close}><Icon name="close" size={16} /></button>
        <Cover id={meta.id} width={92} />
        <div class="who">
          <span class="series">{meta.seriesName}</span>
          <h2>{meta.label}</h2>
          <span class="file code">{meta.fileName}</span>
        </div>
      </header>

      <div class="grid">
        {#each FIELDS as f (f.key)}
          <label class:wide={f.wide} class:short={f.key === "arc_number" || f.key === "arc_count"}>
            <span class="name">
              {f.label}
              {#if edited(f.key)}
                <span class="src mine" title="Edited by you"><Icon name="edit" size={11} /></span>
                <button type="button" class="undo" title="Undo this edit" aria-label="Undo this edit" onclick={() => revert(f.key)}>
                  <Icon name="undo" size={12} />
                </button>
              {:else if fromCv(f.key)}
                <span class="src" title="From Comic Vine"><Icon name="fetch" size={11} /></span>
              {/if}
            </span>
            {#if f.lines}
              <textarea rows={f.lines} bind:value={values[f.key]} placeholder={f.hint ?? ""} spellcheck="false"></textarea>
            {:else}
              <input bind:value={values[f.key]} placeholder={f.hint ?? ""} spellcheck="false" />
            {/if}
          </label>
        {/each}
      </div>

      <footer>
        {#if error}<span class="err">{error}</span>{/if}
        {#if meta.edited.length}
          <button class="ico" title="Undo all my edits" aria-label="Undo all my edits" onclick={revertAll}>
            <Icon name="undo" size={16} />
          </button>
        {/if}
        <span class="spacer"></span>
        <button class="btn" title="Ctrl+Enter" disabled={saving} onclick={save}>{dirty ? "Save" : "Done"}</button>
      </footer>
    {:else if error}
      <p class="err">{error}</p>
    {:else}
      <p class="loading">Loading…</p>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 150;
    display: grid;
    place-items: center;
    background: rgba(6, 6, 6, 0.72);
    backdrop-filter: blur(18px);
    animation: fade 0.2s var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .sheet {
    width: min(820px, calc(100vw - 48px));
    max-height: calc(100vh - 96px);
    overflow-y: auto;
    padding: 28px 32px 22px;
    border-radius: 12px;
    background: var(--raised);
    box-shadow: 0 0 0 1px var(--line), 0 30px 80px rgba(0, 0, 0, 0.7);
    user-select: text;
  }
  header {
    position: relative;
    display: flex;
    gap: 22px;
    align-items: flex-end;
    margin-bottom: 26px;
  }
  .x {
    position: absolute;
    top: -12px;
    right: -16px;
  }
  .who {
    min-width: 0;
  }
  .series {
    color: var(--text-2);
    font-size: 13px;
  }
  h2 {
    margin-top: 4px;
    font-weight: 400;
    font-size: 30px;
    letter-spacing: -0.03em;
  }
  .file {
    display: block;
    margin-top: 6px;
    color: var(--text-3);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 14px 18px;
  }
  label {
    grid-column: span 2;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  label.wide {
    grid-column: span 4;
  }
  label.short {
    grid-column: span 1;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--text-2);
    font-size: 12px;
  }
  .src {
    display: inline-grid;
    color: var(--text-3);
  }
  .src.mine {
    color: var(--text);
  }
  .undo {
    margin-left: auto;
    display: inline-grid;
    color: var(--text-3);
  }
  .undo:hover {
    color: var(--text);
  }
  input,
  textarea {
    width: 100%;
    background: var(--menu);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 8px 10px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    outline: none;
    resize: vertical;
    transition: border-color 0.15s;
  }
  input:focus,
  textarea:focus {
    border-color: var(--text-3);
  }
  input::placeholder,
  textarea::placeholder {
    color: var(--text-3);
  }
  footer {
    display: flex;
    align-items: center;
    gap: 22px;
    margin-top: 24px;
  }
  .spacer {
    flex: 1;
  }
  .err {
    color: var(--text);
    font-size: 13px;
  }
  .loading {
    color: var(--text-2);
  }
  .btn:disabled {
    opacity: 0.5;
  }
</style>
