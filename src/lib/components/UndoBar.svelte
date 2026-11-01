<script lang="ts">
  import { fly } from "svelte/transition";
  import { motionMs } from "$lib/look.svelte";
  import { holdUndo, reader, takeBack, undo } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";
</script>

<!-- What just changed, in a caption box along the bottom, with the way to take it back. -->
{#if undo.text && reader.id === null}
  <div
    class="undo"
    role="status"
    transition:fly={{ y: 20, duration: motionMs(180, 320) }}
    onpointerenter={() => holdUndo(true)}
    onpointerleave={() => holdUndo(false)}
  >
    <span class="text">{undo.text}</span>
    <button class="take" title="Undo (Ctrl+Z)" onclick={takeBack}><Icon name="undo" size={15} stroke={2} />Undo</button>
  </div>
{/if}

<style>
  .undo {
    position: fixed;
    z-index: 30;
    bottom: 28px;
    left: calc(50% + var(--side-now) / 2);
    translate: -50% 0;
    max-width: calc(100vw - var(--side-now) - 48px);
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 8px 8px 8px 16px;
    background: #e6dfd1;
    color: #141414;
    border: 3px solid var(--ink);
  }
  .text {
    font: 400 21px/1 var(--display);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    padding-top: 3px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .take {
    flex: none;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 7px 13px;
    background: #141414;
    color: #e6dfd1;
    font-weight: 500;
    transition: background 0.15s;
  }
  .take:hover {
    background: #000;
  }
</style>
