<script lang="ts">
  // What the reader shows instead of a blank screen: a quiet spinner while a page is on its way,
  // or a message when it can't be loaded.
  import Icon from "$lib/components/Icon.svelte";

  let { waiting, failed = null, onretry }: { waiting: boolean; failed?: number | null; onretry: () => void } = $props();
</script>

{#if failed !== null}
  <div class="status" role="alert">
    <p>Couldn’t load page {failed + 1}.</p>
    <button class="ico" title="Try again" aria-label="Try again" onclick={(e) => (e.stopPropagation(), onretry())}>
      <Icon name="rescan" size={17} />
    </button>
  </div>
{:else if waiting}
  <div class="spinner" aria-label="Loading page"></div>
{/if}

<style>
  .status {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 2;
    text-align: center;
    color: var(--text-2);
  }
  .status .ico {
    margin-top: 10px;
    color: var(--text);
  }
  /* Only appears if the page takes more than a moment, so fast turns never flash it. */
  .spinner {
    position: absolute;
    left: 50%;
    top: 50%;
    z-index: 2;
    width: 22px;
    height: 22px;
    margin: -11px 0 0 -11px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.15);
    border-top-color: rgba(255, 255, 255, 0.7);
    opacity: 0;
    animation:
      show 0.2s 0.35s forwards,
      spin 0.8s linear infinite;
    pointer-events: none;
  }
  @keyframes show {
    to {
      opacity: 1;
    }
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
