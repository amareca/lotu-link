<script lang="ts">
  import AuthHelpModal from "./lib/components/AuthHelpModal.svelte";
  import Drawer from "./lib/components/Drawer.svelte";
  import Rail from "./lib/components/Rail.svelte";
  import Topbar from "./lib/components/Topbar.svelte";
  import DevicesView from "./lib/views/DevicesView.svelte";
  import MirrorView from "./lib/views/MirrorView.svelte";
  import SettingsView from "./lib/views/SettingsView.svelte";
  import type { AppViewModel } from "./lib/viewmodels/AppViewModel.svelte";
  import type { DevicesViewModel } from "./lib/viewmodels/DevicesViewModel.svelte";
  import type { SessionViewModel } from "./lib/viewmodels/SessionViewModel.svelte";

  interface Props {
    app: AppViewModel;
    devices: DevicesViewModel;
    session: SessionViewModel;
  }

  let { app, devices, session }: Props = $props();

  const MIN_WIDTH = 400;
  const MIN_HEIGHT = 640;

  let frameWidth = $state(420);
  let frameHeight = $state(860);
  let resizing = $state(false);
  let resizeOrigin = { x: 0, y: 0, width: 0, height: 0 };
  let authReturnFocus: HTMLElement | null = null;

  const connectedDevice = $derived(
    devices.devices.find((device) => device.state === "connected") ?? null,
  );
  const unauthorizedCount = $derived(
    devices.devices.filter((device) => device.state === "unauthorized").length,
  );

  const connectionLabel = $derived.by(() => {
    if (connectedDevice) return "Conectado";
    if (unauthorizedCount > 0) return "Sin autorizar";
    if (devices.loading && devices.lastScanAt === null) return "Buscando…";
    if (devices.lastScanAt !== null) {
      return devices.devices.length === 0 ? "Sin dispositivos" : "Sin conexión";
    }
    return "USB";
  });

  const connectionTone = $derived(
    connectedDevice ? "ok" : unauthorizedCount > 0 ? "warn" : "muted",
  );

  $effect(() => {
    devices.start();
    return () => devices.dispose();
  });

  $effect(() => {
    if (app.drawer === "devices") {
      void devices.refresh();
    }
  });

  function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
  }

  function viewportWidth(): number {
    return Math.max(MIN_WIDTH, window.innerWidth - 32);
  }

  function viewportHeight(): number {
    return Math.max(MIN_HEIGHT, window.innerHeight - 32);
  }

  function startResize(event: PointerEvent): void {
    if (event.button !== 0) return;

    resizing = true;
    resizeOrigin = {
      x: event.clientX,
      y: event.clientY,
      width: frameWidth,
      height: frameHeight,
    };

    const handle = event.currentTarget;
    if (handle instanceof HTMLElement) {
      handle.setPointerCapture(event.pointerId);
    }
    event.preventDefault();
  }

  function moveResize(event: PointerEvent): void {
    if (!resizing) return;

    frameWidth = clamp(
      resizeOrigin.width + (event.clientX - resizeOrigin.x),
      MIN_WIDTH,
      viewportWidth(),
    );
    frameHeight = clamp(
      resizeOrigin.height + (event.clientY - resizeOrigin.y),
      MIN_HEIGHT,
      viewportHeight(),
    );
  }

  function endResize(event: PointerEvent): void {
    if (!resizing) return;
    resizing = false;

    const handle = event.currentTarget;
    if (
      handle instanceof HTMLElement &&
      handle.hasPointerCapture(event.pointerId)
    ) {
      handle.releasePointerCapture(event.pointerId);
    }
  }

  function resizeWithKeyboard(event: KeyboardEvent): void {
    const step = event.shiftKey ? 24 : 8;

    if (event.key === "ArrowRight") {
      frameWidth = clamp(frameWidth + step, MIN_WIDTH, viewportWidth());
    } else if (event.key === "ArrowLeft") {
      frameWidth = clamp(frameWidth - step, MIN_WIDTH, viewportWidth());
    } else if (event.key === "ArrowDown") {
      frameHeight = clamp(frameHeight + step, MIN_HEIGHT, viewportHeight());
    } else if (event.key === "ArrowUp") {
      frameHeight = clamp(frameHeight - step, MIN_HEIGHT, viewportHeight());
    } else {
      return;
    }

    event.preventDefault();
  }

  function closeDrawer(): void {
    app.closeDrawer();
    document
      .getElementById("rail-mirror")
      ?.focus({ preventScroll: true });
  }

  function openAuthHelp(): void {
    const active = document.activeElement;
    authReturnFocus = active instanceof HTMLElement ? active : null;
    app.openAuthHelp();
  }

  function restoreAuthFocus(): void {
    authReturnFocus?.focus({ preventScroll: true });
    authReturnFocus = null;
  }

  function closeAuthHelp(): void {
    app.closeAuthHelp();
    restoreAuthFocus();
  }

  /** Opens a live session for a connected device and returns to the mirror. */
  function openMirror(serial: string): void {
    void session.startSession(serial);
    closeDrawer();
  }

  /** Dev-only replay control: starts the fake/real session and shows the mirror. */
  function startReplay(): void {
    void session.startReplay();
    closeDrawer();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;

    const authWasOpen = app.authHelpOpen;
    if (!app.handleEscape()) return;

    event.preventDefault();

    if (authWasOpen) {
      restoreAuthFocus();
    } else {
      document
        .getElementById("rail-mirror")
        ?.focus({ preventScroll: true });
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#snippet settingsFooter()}
  <button class="btn ghost" type="button" disabled>Cancelar</button>
  <button class="btn primary" type="button" disabled>Guardar</button>
{/snippet}

{#snippet devicesFooter()}
  <button
    class="btn primary"
    type="button"
    onclick={startReplay}
    disabled={session.phase === "connecting"}
    title="Reproduce el fixture H.264 por el WebSocket interno"
  >
    Replay (dev)
  </button>
{/snippet}

<div
  class="workspace"
  class:drawer-open={app.drawer !== null}
  class:resizing
  style={`--frame-w:${frameWidth}px;--frame-h:${frameHeight}px`}
>
  <div class="drawer-slot">
    <Drawer
      id="devices"
      title="Dispositivos"
      open={app.drawer === "devices"}
      onClose={closeDrawer}
      footer={import.meta.env.DEV ? devicesFooter : undefined}
    >
      <DevicesView {devices} onAuthHelp={openAuthHelp} onOpenMirror={openMirror} />
    </Drawer>

    <Drawer
      id="settings"
      title="Ajustes"
      open={app.drawer === "settings"}
      onClose={closeDrawer}
      footer={settingsFooter}
    >
      <SettingsView />
    </Drawer>
  </div>

  <div class="app-frame">
    <Topbar label={connectionLabel} tone={connectionTone} />

    <div class="app-body">
      <Rail {app} {session} />
      <MirrorView {session} />

      {#if app.drawer !== null}
        <button
          class="scrim"
          type="button"
          aria-label="Cerrar panel"
          onclick={closeDrawer}
        ></button>
      {/if}
    </div>

    <button
      class="resize-handle"
      type="button"
      title="Arrastra o usa las flechas para redimensionar"
      aria-label="Redimensionar marco de la aplicación"
      onpointerdown={startResize}
      onpointermove={moveResize}
      onpointerup={endResize}
      onpointercancel={endResize}
      onkeydown={resizeWithKeyboard}
    >
      <svg
        width="15"
        height="15"
        viewBox="0 0 15 15"
        fill="none"
        stroke="currentColor"
        stroke-width="1.7"
        stroke-linecap="round"
        aria-hidden="true"
      >
        <path d="M13 6L6 13M13 10L10 13" />
      </svg>
    </button>
  </div>
</div>

{#if app.authHelpOpen}
  <AuthHelpModal onClose={closeAuthHelp} />
{/if}

<style>
  .workspace {
    position: relative;
    isolation: isolate;
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100dvh;
    padding: 16px;
  }

  .workspace.resizing {
    cursor: nwse-resize;
    user-select: none;
  }

  .drawer-slot {
    position: relative;
    flex: none;
    width: 0;
    height: min(var(--frame-h), calc(100dvh - 32px));
    margin-right: 0;
    overflow: hidden;
    transition:
      width 220ms ease,
      margin-right 220ms ease;
  }

  .workspace.drawer-open .drawer-slot {
    width: clamp(240px, calc(100vw - 484px), 340px);
    margin-right: 16px;
  }

  .app-frame {
    position: relative;
    container: app / inline-size;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    flex: 0 1 auto;
    width: var(--frame-w);
    height: var(--frame-h);
    min-width: 400px;
    max-width: 100%;
    max-height: calc(100dvh - 32px);
    overflow: hidden;
    background: var(--app);
    border: 1px solid var(--border);
    border-radius: 20px;
    box-shadow:
      0 34px 90px rgba(0, 0, 0, 0.66),
      0 0 0 1px rgba(0, 0, 0, 0.45),
      0 0 60px rgba(163, 230, 53, 0.035),
      inset 0 1px 0 rgba(255, 255, 255, 0.035);
  }

  .app-body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: stretch;
    overflow: hidden;
  }

  .scrim {
    display: none;
  }

  .resize-handle {
    position: absolute;
    right: 4px;
    bottom: 4px;
    z-index: 65;
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    color: var(--dim);
    border-radius: 8px;
    cursor: nwse-resize;
    touch-action: none;
    transition:
      color var(--transition),
      background var(--transition);
  }

  .resize-handle:hover {
    color: var(--lime);
    background: rgba(163, 230, 53, 0.1);
  }

  @media (max-width: 719px) {
    .workspace.drawer-open .drawer-slot {
      position: absolute;
      top: 16px;
      bottom: 16px;
      left: 16px;
      z-index: 60;
      width: min(340px, calc(100vw - 32px));
      height: auto;
      margin-right: 0;
      overflow: hidden;
    }

    .scrim {
      position: absolute;
      inset: 0;
      z-index: 30;
      display: block;
      width: 100%;
      height: 100%;
      border-radius: 20px;
      background: rgba(3, 7, 14, 0.6);
      cursor: default;
    }
  }
</style>
