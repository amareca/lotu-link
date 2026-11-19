<script lang="ts">
  import type { AppViewModel } from "../viewmodels/AppViewModel.svelte";
  import type { SessionViewModel } from "../viewmodels/SessionViewModel.svelte";

  interface Props {
    app: AppViewModel;
    session: SessionViewModel;
  }

  let { app, session }: Props = $props();
</script>

<nav class="rail" aria-label="Acciones principales">
  <button
    id="rail-devices"
    class="rail-btn"
    class:active={app.drawer === "devices"}
    type="button"
    data-tip="Dispositivos"
    aria-label="Dispositivos"
    aria-expanded={app.drawer === "devices"}
    aria-controls="drawer-devices"
    onclick={() => app.toggleDrawer("devices")}
  >
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <rect x="7" y="2.5" width="10" height="19" rx="2.5" />
      <path d="M11 18.5h2" />
    </svg>
  </button>

  <button
    id="rail-mirror"
    class="rail-btn"
    class:active={app.drawer === null}
    type="button"
    data-tip="Espejo"
    aria-label="Volver al espejo"
    onclick={() => app.closeDrawer()}
  >
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <rect x="2.5" y="4" width="19" height="13" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </svg>
  </button>

  <span class="rail-sep" aria-hidden="true"></span>

  <button
    class="rail-btn"
    type="button"
    data-tip="Capturar pantalla"
    aria-label="Capturar pantalla"
    title="Capturar pantalla (PNG)"
    disabled={!session.inputEnabled}
    onclick={() => void session.capture()}
  >
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path
        d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2"
      />
      <circle cx="12" cy="12" r="3" />
    </svg>
  </button>

  <button
    class="rail-btn"
    class:recording={session.recording?.active}
    type="button"
    data-tip={session.recording?.active ? "Detener grabación" : "Grabar vídeo"}
    aria-label={session.recording?.active ? "Detener grabación" : "Grabar vídeo"}
    title={session.recording?.active ? "Detener grabación" : "Grabar vídeo (MP4)"}
    disabled={!session.inputEnabled}
    onclick={() => void (session.recording?.active ? session.stopRecording() : session.startRecording())}
  >
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      aria-hidden="true"
    >
      {#if session.recording?.active}
        <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none" />
      {:else}
        <circle cx="12" cy="12" r="8.5" />
        <circle cx="12" cy="12" r="3.6" fill="currentColor" stroke="none" />
      {/if}
    </svg>
  </button>

  <button
    id="rail-settings"
    class="rail-btn"
    class:active={app.drawer === "settings"}
    type="button"
    data-tip="Ajustes"
    aria-label="Ajustes"
    aria-expanded={app.drawer === "settings"}
    aria-controls="drawer-settings"
    onclick={() => app.toggleDrawer("settings")}
  >
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path
        d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1 1.55V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1-1.55 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.55-1H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.55-1 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34h.01a1.7 1.7 0 0 0 1-1.55V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1 1.55h.01a1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.01a1.7 1.7 0 0 0 1.55 1H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.55 1z"
      />
    </svg>
  </button>

  <div class="rail-foot">
    <span class="dot" title="Conexión USB" aria-hidden="true"></span>
    <span class="rail-version">v0.1</span>
  </div>
</nav>

<style>
  .rail {
    width: 56px;
    flex: none;
    position: relative;
    z-index: 45;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 10px 0;
    background: var(--rail);
    border-right: 1px solid var(--border-soft);
  }

  .rail-btn {
    position: relative;
    width: 40px;
    height: 40px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 11px;
    color: var(--muted);
    transition:
      background var(--transition),
      color var(--transition);
  }

  .rail-btn:hover:not(:disabled) {
    background: #121e32;
    color: var(--text);
  }

  .rail-btn:active:not(:disabled) {
    transform: translateY(1px);
  }

  .rail-btn:disabled {
    opacity: 0.45;
  }

  .rail-btn.recording {
    color: var(--danger);
    background: var(--danger-soft);
  }

  .rail-btn.active {
    background: var(--lime-soft);
    color: var(--lime);
  }

  .rail-btn.active::before {
    content: "";
    position: absolute;
    left: -8px;
    top: 10px;
    bottom: 10px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--lime);
  }

  .rail-btn::after {
    content: attr(data-tip);
    position: absolute;
    left: calc(100% + 10px);
    top: 50%;
    transform: translate(-4px, -50%);
    padding: 6px 10px;
    border-radius: 8px;
    font-size: 16px;
    font-weight: 600;
    color: var(--text);
    background: #0b1220;
    border: 1px solid var(--border);
    box-shadow: 0 8px 22px rgba(0, 0, 0, 0.45);
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    visibility: hidden;
    transition:
      opacity var(--transition),
      transform var(--transition),
      visibility var(--transition);
    z-index: 80;
  }

  .rail-btn:hover::after,
  .rail-btn:focus-visible::after {
    opacity: 1;
    visibility: visible;
    transform: translate(0, -50%);
  }

  .rail-sep {
    width: 22px;
    height: 1px;
    flex: none;
    background: var(--border-soft);
    margin: 3px 0;
  }

  .rail-foot {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    padding-top: 8px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--lime);
    box-shadow: 0 0 8px rgba(163, 230, 53, 0.6);
  }

  .rail-version {
    font-size: 16px;
    letter-spacing: 0.2px;
    color: #475569;
    font-weight: 600;
  }
</style>
