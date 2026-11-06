<script lang="ts">
  // Taking books, a series or a library out, always asked first. Hiding leaves the files alone and
  // can be undone in Settings; deleting moves them to the Recycle Bin, after one more question.
  import { api } from "$lib/api";
  import { removing } from "$lib/manage.svelte";
  import { lib, nav } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";

  let confirming = $state(false);
  let busy = $state(false);
  let error = $state("");
  const t = $derived(removing.target);

  function close() {
    removing.target = null;
    confirming = false;
    error = "";
  }
  async function act(del: boolean) {
    if (!t || busy) return;
    busy = true;
    error = "";
    try {
      await api.removeBooks(t, del);
      const leaves = t.leaves;
      close();
      lib.version++;
      if (leaves) nav.root({ name: "home" });
    } catch (e) {
      error = String(e);
      lib.version++;
    } finally {
      busy = false;
    }
  }
  function keys(e: KeyboardEvent) {
    if (t && e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (confirming) confirming = false;
      else close();
    }
  }
  function focus(el: HTMLElement) {
    el.focus();
  }
</script>

<svelte:window onkeydowncapture={keys} />

{#if t}
  <div class="back" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="card" role="dialog" aria-modal="true" aria-label="Remove {t.title}">
      {#if !confirming}
        <h2>Remove {t.title}</h2>
        <p class="detail fine">{t.detail}</p>
        <button class="way" disabled={busy} use:focus onclick={() => act(false)}>
          <Icon name="close" size={16} />
          <span>Hide it<small>Its files stay where they are. Put it back any time in Settings.</small></span>
        </button>
        <button class="way" disabled={busy} onclick={() => (confirming = true)}>
          <Icon name="alert" size={16} />
          <span>Delete it<small>Moves {t.what} to the Recycle Bin.</small></span>
        </button>
      {:else}
        <h2>Delete {t.title}?</h2>
        <p class="detail fine">{t.detail}</p>
        <p class="sure">
          This moves {t.what} to the Recycle Bin and forgets its details, bookmarks and where you'd read to. On a drive
          with no Recycle Bin, Windows may delete {t.what} for good.
        </p>
      {/if}
      {#if error}<p class="err"><Icon name="alert" size={14} />{error}</p>{/if}
      <div class="choices">
        {#if confirming}
          <button class="link" use:focus onclick={() => (confirming = false)}>Back</button>
          <button class="btn" disabled={busy} onclick={() => act(true)}>{busy ? "Deleting…" : "Delete"}</button>
        {:else}
          <button class="link" onclick={close}>Cancel</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .back {
    position: fixed;
    inset: 0;
    z-index: 300;
    display: grid;
    place-items: center;
    background: rgba(6, 6, 6, 0.78);
    backdrop-filter: blur(14px);
  }
  .card {
    width: min(460px, calc(100vw - 48px));
    padding: 30px 32px 26px;
    border-radius: 14px;
    box-shadow: 0 0 0 1px var(--line);
    background: var(--raised);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h2 {
    margin: 0;
    font-size: 19px;
    font-weight: 500;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }
  .detail {
    margin: 0 0 8px;
    color: var(--text-3);
    font-size: 12px;
  }
  .way {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 13px 16px;
    border-radius: 10px;
    text-align: left;
    box-shadow: inset 0 0 0 1px var(--line);
    transition: background 0.15s;
  }
  .way :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--text-2);
  }
  .way:hover:not(:disabled) {
    background: rgb(var(--fg) / 0.07);
  }
  .way span {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .way small {
    color: var(--text-3);
    font-size: 13px;
    line-height: 1.4;
  }
  .sure {
    margin: 0;
    color: var(--text-2);
    font-size: 14px;
    line-height: 1.5;
  }
  .err {
    display: flex;
    gap: 8px;
    margin: 4px 0 0;
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .err :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .choices {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 22px;
    margin-top: 12px;
    min-height: 40px;
  }
  .btn:disabled {
    opacity: 0.5;
    pointer-events: none;
  }
</style>
