<script lang="ts">
  // What's new: the change log of a version waiting to install (with the way to install it), or of
  // the one just installed, shown once on its first start.
  import Icon from "./Icon.svelte";
  import { install, noteSections, notesFor, shortVersion, updates } from "$lib/updates.svelte";

  const after = $derived(updates.sheet === "after");
  const sections = $derived(noteSections(after ? notesFor(updates.current) : (updates.next?.notes ?? "")));
  const when = $derived.by(() => {
    // However the date comes, its day is "2026-11-20" somewhere in it.
    const day = /\d{4}-\d{2}-\d{2}/.exec(updates.next?.date ?? "")?.[0];
    const d = day ? new Date(`${day}T12:00:00`) : null;
    return d && !isNaN(+d) ? d.toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" }) : null;
  });

  const close = () => (updates.sheet = null);
  function onkey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    close();
  }
</script>

<svelte:window onkeydowncapture={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="sheet" role="dialog" aria-label="What's new">
    <div class="caption">{after ? "Updated" : "What's new"}</div>
    <button class="ico x" title="Close (Esc)" aria-label="Close" onclick={close}><Icon name="close" size={16} /></button>
    <div class="head">
      {#if after}
        <h2>You're on {shortVersion(updates.current)}</h2>
        <p class="fine">Here's what changed. Your library, progress and settings are as you left them.</p>
      {:else if updates.next}
        <h2>AionComics {shortVersion(updates.next.version)}</h2>
        <p class="fine">{when ? `Out ${when} · ` : ""}you have {updates.current}</p>
      {/if}
    </div>
    <div class="body">
      {#each sections as s (s.title)}
        <h3>{s.title}</h3>
        <ul>
          {#each s.items as it, i (i)}
            <li>{#if it.lead}<b>{it.lead}</b>{" "}{/if}{it.rest}</li>
          {/each}
        </ul>
      {:else}
        <p class="fine">No notes came with this version.</p>
      {/each}
    </div>
    <div class="foot">
      {#if after}
        <span class="grow"></span>
        <button class="pill" onclick={close}><Icon name="check" size={15} />Got it</button>
      {:else if updates.portable}
        <span class="fine">This copy runs without installing, so it can't update itself. Download the new one and use it instead.</span>
        <span class="grow"></span>
        <button class="link" onclick={close}>Later</button>
        <button class="pill" onclick={install}><Icon name="arrived" size={15} />Download</button>
      {:else if updates.phase === "downloading"}
        <div class="bar">
          <span>Downloading</span>
          <span class="track"><i style:width="{Math.round(updates.progress * 100)}%"></i></span>
          <span>{Math.round(updates.progress * 100)}%</span>
        </div>
        <button class="link" onclick={close}>Hide</button>
      {:else if updates.phase === "installing"}
        <span class="fine">Installing. AionComics opens again in a moment.</span>
      {:else if updates.phase === "error"}
        <span class="fine err"><Icon name="alert" size={14} />{updates.error || "The update didn't download."}</span>
        <span class="grow"></span>
        <button class="pill" onclick={install}><Icon name="rescan" size={15} />Try again</button>
      {:else}
        <span class="fine">Takes a few seconds. AionComics opens again afterward.</span>
        <span class="grow"></span>
        <button class="link" onclick={close}>Later</button>
        <button class="pill" onclick={install}><Icon name="rescan" size={15} />Restart and update</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 160;
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
  /* A comic panel: inked frame, its caption box on the corner. */
  .sheet {
    position: relative;
    width: min(720px, calc(100vw - 48px));
    max-height: calc(100vh - 96px);
    display: flex;
    flex-direction: column;
    background: var(--raised);
    border: 4px solid var(--ink);
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.7);
  }
  .sheet > .caption {
    position: absolute;
    top: -4px;
    left: -4px;
  }
  .x {
    position: absolute;
    top: 10px;
    right: 10px;
    color: var(--text-3);
  }
  .head {
    padding: 58px 36px 18px;
  }
  h2 {
    margin: 0;
    font: 400 64px/0.9 var(--display);
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }
  .head .fine {
    margin: 10px 0 0;
    color: var(--text-3);
    font-size: 13px;
  }
  .body {
    overflow: auto;
    padding: 4px 36px 10px;
    border-top: 1px solid var(--line);
  }
  h3 {
    margin: 22px 0 8px;
    color: var(--text-3);
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    position: relative;
    padding: 0 0 10px 16px;
    color: var(--text-2);
    font-size: 14px;
    line-height: 1.55;
  }
  li::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0.72em;
    width: 5px;
    height: 5px;
    background: var(--text-3);
  }
  li b {
    color: var(--text);
    font-weight: 500;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 16px 36px 20px;
    border-top: 1px solid var(--line);
  }
  .foot .fine {
    color: var(--text-3);
    font-size: 12px;
  }
  .err {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .grow {
    flex: 1;
  }
  .pill {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 9px;
    height: 40px;
    padding: 0 20px;
    border-radius: 999px;
    background: var(--text);
    color: var(--bg);
    font-size: 14px;
  }
  .pill:hover {
    opacity: 0.9;
  }
  .bar {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 14px;
    color: var(--text-2);
    font-size: 13px;
  }
  .track {
    flex: 1;
    height: 3px;
    position: relative;
    background: rgb(var(--fg) / 0.1);
  }
  .track i {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--text);
    transition: width 0.2s linear;
  }
</style>
