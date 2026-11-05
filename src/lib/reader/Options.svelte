<script lang="ts">
  import Icon from "$lib/components/Icon.svelte";
  import { FIT_LABEL, type Ctl } from "./ctl.svelte";
  import { prefs, savePrefs, type Prefs } from "./prefs.svelte";

  let { c }: { c: Ctl } = $props();

  function set<K extends keyof Prefs>(k: K, v: Prefs[K]) {
    prefs[k] = v;
    savePrefs();
  }
  const adjusted = $derived(prefs.brightness !== 100 || prefs.contrast !== 100);
  function resetPage() {
    set("brightness", 100);
    set("contrast", 100);
  }
</script>

<aside class="panel">
  <h2>Options</h2>

  <div class="row">
    <span class="with-reset">
      Page
      {#if adjusted}
        <button class="ico" title="Back to normal" aria-label="Back to normal" onclick={resetPage}><Icon name="undo" size={15} /></button>
      {/if}
    </span>
    <label class="slide">
      <span>Brightness</span>
      <input type="range" min="50" max="150" step="5" value={prefs.brightness} oninput={(e) => set("brightness", +e.currentTarget.value)} />
      <span class="mono">{prefs.brightness}%</span>
    </label>
    <label class="slide">
      <span>Contrast</span>
      <input type="range" min="50" max="150" step="5" value={prefs.contrast} oninput={(e) => set("contrast", +e.currentTarget.value)} />
      <span class="mono">{prefs.contrast}%</span>
    </label>
  </div>

  <div class="row">
    <span>Background</span>
    <div class="choice">
      <button class:on={prefs.background === "black"} onclick={() => set("background", "black")}>Black</button>
      <button class:on={prefs.background === "gray"} onclick={() => set("background", "gray")}>Dark gray</button>
    </div>
  </div>

  <div class="row">
    <span>Page fit</span>
    <div class="choice">
      {#each Object.entries(FIT_LABEL) as [k, label] (k)}
        <button class:on={c.view.fit === k} onclick={() => c.setFit(k as Prefs["fit"])}>{label.replace(/^Fit (.)/, (_, ch) => ch.toUpperCase())}</button>
      {/each}
    </div>
  </div>

  <div class="row">
    <span>Guided View speed</span>
    <div class="choice">
      <button class:on={prefs.guidedSpeed === "slow"} onclick={() => set("guidedSpeed", "slow")}>Slow</button>
      <button class:on={prefs.guidedSpeed === "normal"} onclick={() => set("guidedSpeed", "normal")}>Normal</button>
      <button class:on={prefs.guidedSpeed === "fast"} onclick={() => set("guidedSpeed", "fast")}>Fast</button>
    </div>
  </div>

  <div class="row">
    <span>Each new page</span>
    <div class="choice">
      <button class:on={prefs.guidedOverview} onclick={() => set("guidedOverview", true)}>Whole page first</button>
      <button class:on={!prefs.guidedOverview} onclick={() => set("guidedOverview", false)}>Straight to panels</button>
    </div>
  </div>

  <div class="row">
    <span>Tall panels</span>
    <div class="choice">
      <button class:on={prefs.guidedParts} onclick={() => set("guidedParts", true)}>Top to bottom</button>
      <button class:on={!prefs.guidedParts} onclick={() => set("guidedParts", false)}>Whole</button>
    </div>
  </div>

  <div class="row">
    <span>Around the panel</span>
    <div class="choice">
      <button class:on={prefs.guidedDim === "light"} onclick={() => set("guidedDim", "light")}>Dim</button>
      <button class:on={prefs.guidedDim === "strong"} onclick={() => set("guidedDim", "strong")}>Dark</button>
      <button class:on={prefs.guidedDim === "hidden"} onclick={() => set("guidedDim", "hidden")}>Hide</button>
    </div>
  </div>

  <div class="row">
    <span>Page number and clock</span>
    <div class="choice">
      <button class:on={!prefs.info} onclick={() => set("info", false)}>With toolbar</button>
      <button class:on={prefs.info} onclick={() => set("info", true)}>Always</button>
    </div>
  </div>
</aside>

<style>
  .panel {
    position: absolute;
    top: 64px;
    right: 20px;
    z-index: 5;
    width: 360px;
    max-height: calc(100vh - 84px);
    overflow-y: auto;
    padding: 22px 24px 8px;
    border-radius: 10px;
    background: rgba(20, 20, 20, 0.92);
    backdrop-filter: blur(20px);
    box-shadow: 0 0 0 1px var(--line), 0 24px 60px rgba(0, 0, 0, 0.6);
    animation: slide 0.25s var(--ease);
  }
  @keyframes slide {
    from {
      opacity: 0;
      transform: translateX(12px);
    }
  }
  h2 {
    font-weight: 400;
    font-size: 15px;
    margin-bottom: 8px;
  }
  .row {
    padding: 14px 0;
    border-top: 1px solid var(--line);
  }
  .row:first-of-type {
    border-top: 0;
  }
  .row > span {
    display: block;
    color: var(--text-2);
    font-size: 13px;
    margin-bottom: 10px;
  }
  .row > .with-reset {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 24px;
  }
  .slide {
    display: grid;
    grid-template-columns: 76px 1fr 42px;
    align-items: center;
    gap: 10px;
    padding: 4px 0;
    font-size: 13px;
    color: var(--text-2);
  }
  .slide .mono {
    text-align: right;
    font-size: 11px;
  }
  .slide input {
    width: 100%;
    min-width: 0;
    accent-color: var(--text);
  }
  .choice {
    display: flex;
    gap: 18px;
  }
  .choice button {
    color: var(--text-3);
    transition: color 0.2s;
  }
  .choice button:hover {
    color: var(--text-2);
  }
  .choice button.on {
    color: var(--text);
  }
</style>
