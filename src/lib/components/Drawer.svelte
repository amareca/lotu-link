<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    id: string;
    title: string;
    open: boolean;
    onClose: () => void;
    children: Snippet;
    footer?: Snippet;
  }

  let { id, title, open, onClose, children, footer }: Props = $props();

  let closeButton = $state<HTMLButtonElement | null>(null);
  let wasOpen = false;

  $effect(() => {
    if (open === wasOpen) return;
    wasOpen = open;

    if (!open) return;

    const frame = requestAnimationFrame(() => {
      closeButton?.focus({ preventScroll: true });
    });

    return () => cancelAnimationFrame(frame);
  });
</script>

<div
  id={`drawer-${id}`}
  class="drawer"
  class:open
  role="dialog"
  aria-modal="false"
  aria-labelledby={`${id}-title`}
  aria-hidden={!open}
  inert={!open}
>
  <header class="drawer-head">
    <h2 id={`${id}-title`}>{title}</h2>
    <button
      class="icon-btn"
      type="button"
      bind:this={closeButton}
      onclick={onClose}
      aria-label={`Cerrar ${title.toLocaleLowerCase("es")}`}
      title="Cerrar"
    >
      <svg
        width="17"
        height="17"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        aria-hidden="true"
      >
        <path d="M5 5l14 14M19 5L5 19" />
      </svg>
    </button>
  </header>

  <div class="drawer-body">
    {@render children()}
  </div>

  {#if footer}
    <footer class="drawer-foot">
      {@render footer()}
    </footer>
  {/if}
</div>

<style>
  .drawer {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    background: linear-gradient(180deg, #162033 0%, #121c2e 100%);
    border: 1px solid var(--border);
    border-radius: 20px;
    box-shadow:
      0 24px 60px rgba(0, 0, 0, 0.45),
      0 0 0 1px rgba(0, 0, 0, 0.35);
    opacity: 0;
    pointer-events: none;
    transition: opacity 160ms ease;
  }

  .drawer.open {
    opacity: 1;
    pointer-events: auto;
  }

  .drawer-head {
    flex: none;
    min-height: 58px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px 0 16px;
    border-bottom: 1px solid var(--border-soft);
    background: rgba(11, 18, 32, 0.42);
  }

  .drawer-head h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 18px;
    font-weight: 650;
    letter-spacing: 0.1px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .drawer-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    scrollbar-width: thin;
    scrollbar-color: #2b3a52 transparent;
  }

  .drawer-foot {
    flex: none;
    display: flex;
    gap: 10px;
    padding: 12px 14px;
    border-top: 1px solid var(--border-soft);
    background: rgba(11, 18, 32, 0.55);
  }

  .drawer-foot :global(.btn) {
    flex: 1;
  }

  @media (max-width: 719px) {
    .drawer {
      border-radius: 20px 0 0 20px;
    }
  }
</style>
