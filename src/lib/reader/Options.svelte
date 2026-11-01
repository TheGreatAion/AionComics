<script lang="ts">
  import { FIT_LABEL, type Ctl } from "./ctl.svelte";
  import { prefs, savePrefs, type Prefs } from "./prefs.svelte";

  let { c }: { c: Ctl } = $props();

  function set<K extends keyof Prefs>(k: K, v: Prefs[K]) {
    prefs[k] = v;
    savePrefs();
  }
</script>

<aside class="panel">
  <h2>Options</h2>

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
