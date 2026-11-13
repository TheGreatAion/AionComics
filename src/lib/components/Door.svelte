<script lang="ts">
  import { door } from "$lib/api";
  import { lib as library } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";

  /** `empty`: in, but AionComics on the PC has no comics folder yet. */
  let { empty = false }: { empty?: boolean } = $props();

  let code = $state("");
  let problem = $state("");
  let busy = $state(false);

  async function enter() {
    if (busy || code.length !== 6) return;
    busy = true;
    problem = "";
    try {
      await door.enter(code);
      // Everything on screen was asked for before the code was in.
      library.version++;
    } catch (e) {
      problem = String(e);
      code = "";
    } finally {
      busy = false;
    }
  }

  // Six digits are the whole code: no need to press anything.
  $effect(() => {
    if (code.length === 6) enter();
  });
</script>

<div class="door">
  <h1>AionComics</h1>
  {#if empty}
    <p>AionComics on your PC doesn't have a comics folder yet. Choose one there, then come back here.</p>
  {:else}
    <p>Type the code AionComics shows on your PC, in Settings under Other devices.</p>
    <label class="field">
      <Icon name="key" size={16} />
      <input
        bind:value={code}
        oninput={() => (code = code.replace(/\D/g, "").slice(0, 6))}
        inputmode="numeric"
        autocomplete="one-time-code"
        maxlength="6"
        placeholder="000000"
        aria-label="Code"
        disabled={busy}
      />
    </label>
    {#if problem}<p class="problem">{problem}</p>{/if}
    <p class="fine note">You only type it once on each device.</p>
  {/if}
</div>

<style>
  .door {
    min-height: 100vh;
    min-height: 100dvh;
    display: grid;
    align-content: center;
    justify-items: center;
    gap: 20px;
    padding: 32px 24px;
    text-align: center;
  }
  h1 {
    font: 400 56px/1 var(--display);
    letter-spacing: 0.03em;
  }
  p {
    max-width: 340px;
    color: var(--text-2);
  }
  .field {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 18px;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--text-3);
  }
  input {
    width: 7.5ch;
    padding: 14px 0;
    background: none;
    border: 0;
    outline: none;
    font: 400 26px/1 var(--mono);
    letter-spacing: 0.12em;
    color: var(--text);
  }
  input::placeholder {
    color: var(--track);
  }
  .problem {
    color: var(--text);
  }
  .note {
    color: var(--text-3);
  }
</style>
