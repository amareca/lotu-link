<script lang="ts">
  import ErrorBanner from "../components/ErrorBanner.svelte";
  import { errorMessageForCode } from "../components/errorMessages";
  import {
    readSystemClipboard,
    writeSystemClipboard,
  } from "../models/systemClipboard";
  import type {
    KeyEventInput,
    WheelDeltaMode,
  } from "../models/input/inputMapper";
  import type {
    PointerEventData,
    SessionViewModel,
    WheelEventData,
  } from "../viewmodels/SessionViewModel.svelte";

  interface Props {
    session: SessionViewModel;
  }

  let { session }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  const active = $derived(session.phase !== "idle");

  const resolutionLabel = $derived(
    session.resolution
      ? `${session.resolution.width}×${session.resolution.height}`
      : "—",
  );

  const liveError = $derived(
    session.phase === "error" && session.origin === "live"
      ? session.error
      : null,
  );

  $effect(() => {
    const element = canvas;
    if (element === null) return;

    return session.attachCanvas(element);
  });

  const inputNoticeText = $derived(
    session.inputNotice === null ? null : errorMessageForCode(session.inputNotice.code),
  );

  function showToast(text: string): void {
    toast = text;
    if (toastTimer !== null) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast = null;
      toastTimer = null;
    }, 2800);
  }

  let lastInputNotice: string | null = null;
  $effect(() => {
    const text = inputNoticeText;
    if (text !== null && text !== lastInputNotice) showToast(text);
    lastInputNotice = text;
    return () => {
      if (toastTimer !== null) clearTimeout(toastTimer);
    };
  });

  function pointerData(event: PointerEvent): PointerEventData | null {
    const element = canvas;
    if (element === null) return null;
    const rect = element.getBoundingClientRect();
    return {
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
      canvasWidth: rect.width,
      canvasHeight: rect.height,
      pointerId: event.pointerId,
      button: event.button,
    };
  }

  function onPointerDown(event: PointerEvent): void {
    if (!session.inputEnabled) return;
    const data = pointerData(event);
    if (data === null) return;

    canvas?.focus();
    if (event.button === 0) {
      canvas?.setPointerCapture(event.pointerId);
    }
    session.pointerDown(data);
  }

  function onPointerMove(event: PointerEvent): void {
    const data = pointerData(event);
    if (data !== null) session.pointerMove(data);
  }

  function onPointerUp(event: PointerEvent): void {
    const data = pointerData(event);
    if (data !== null) session.pointerUp(data);
    if (canvas?.hasPointerCapture(event.pointerId)) {
      canvas.releasePointerCapture(event.pointerId);
    }
  }

  function onPointerCancel(event: PointerEvent): void {
    const data = pointerData(event);
    if (data !== null) session.pointerCancel(data);
  }

  function onWheel(event: WheelEvent): void {
    if (!session.inputEnabled) return;
    const element = canvas;
    if (element === null) return;
    event.preventDefault();

    const rect = element.getBoundingClientRect();
    const data: WheelEventData = {
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
      canvasWidth: rect.width,
      canvasHeight: rect.height,
      deltaX: event.deltaX,
      deltaY: event.deltaY,
      deltaMode: event.deltaMode as WheelDeltaMode,
    };
    session.wheel(data);
  }

  function keyInput(event: KeyboardEvent): KeyEventInput {
    return {
      key: event.key,
      location: event.location,
      shiftKey: event.shiftKey,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
      metaKey: event.metaKey,
      isComposing: event.isComposing || event.keyCode === 229,
      capsLock: event.getModifierState("CapsLock"),
      altGraph: event.getModifierState("AltGraph"),
    };
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (!session.inputEnabled) return;

    // Clipboard shortcuts are local commands, not text sent to the device.
    if (event.ctrlKey && !event.altKey && !event.metaKey) {
      const key = event.key.toLowerCase();
      if (key === "c") {
        event.preventDefault();
        void copyFromDevice();
        return;
      }
      if (key === "v") {
        event.preventDefault();
        void pasteToDevice();
        return;
      }
    }

    if (session.keyDown(keyInput(event))) event.preventDefault();
  }

  function onKeyUp(event: KeyboardEvent): void {
    if (session.keyUp(keyInput(event))) event.preventDefault();
  }

  async function copyFromDevice(): Promise<void> {
    showToast("Leyendo portapapeles del móvil…");
    const text = await session.copyText();
    if (text === null) {
      showToast("No se pudo copiar del móvil.");
      return;
    }
    try {
      await writeSystemClipboard(text);
      showToast("Texto copiado del móvil.");
    } catch {
      showToast("No se pudo escribir en el portapapeles.");
    }
  }

  async function pasteToDevice(): Promise<void> {
    showToast("Leyendo portapapeles del PC…");
    try {
      const text = await readSystemClipboard();
      if (text.length === 0) {
        showToast("El portapapeles está vacío.");
        return;
      }
      session.pasteText(text);
      showToast("Texto pegado en el móvil.");
    } catch {
      showToast("No se pudo leer el portapapeles.");
    }
  }
</script>

<main class="mirror" aria-label="Espejo del dispositivo">
  <div class="session-topbar">
    <span
      class="live-dot"
      class:idle={session.phase === "idle" || session.phase === "error"}
      aria-hidden="true"
    ></span>
    <span class="device-name" title={session.device?.name ?? "Sin sesión"}>
      {session.device?.name ?? "Sin sesión"}
    </span>
    <span class="pill" title="Conexión USB y resolución del stream">
      <span class="pill-usb">USB</span>
      <span class="pill-sep" aria-hidden="true">·</span>
      <span>{resolutionLabel}</span>
    </span>
    {#if session.phase === "streaming"}
      <span class="mini-stat">{session.fps} FPS</span>
    {/if}
    {#if session.phase !== "idle"}
      <button
        class="stop-btn"
        type="button"
        onclick={() => session.stop()}
        title="Detener la sesión"
        aria-label="Detener la sesión"
      >
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="currentColor"
          aria-hidden="true"
        >
          <rect x="5" y="5" width="14" height="14" rx="2.5" />
        </svg>
        Detener
      </button>
    {/if}
  </div>

  {#if liveError !== null}
    <div class="error-slot">
      <ErrorBanner error={liveError} onRetry={() => void session.retry()} />
    </div>
  {/if}

  <section class="stage" aria-label="Vídeo del espejo">
    {#if active}
      <canvas
        bind:this={canvas}
        class="screen"
        tabindex={session.inputEnabled ? 0 : -1}
        aria-label="Vídeo decodificado del dispositivo"
        onpointerdown={onPointerDown}
        onpointermove={onPointerMove}
        onpointerup={onPointerUp}
        onpointercancel={onPointerCancel}
        onwheel={onWheel}
        oncontextmenu={(event) => event.preventDefault()}
        onkeydown={onKeyDown}
        onkeyup={onKeyUp}
        oncompositionend={(event) => session.compositionEnd(event.data)}
      ></canvas>

      {#if session.phase === "connecting"}
        <div class="overlay" role="status">
          <div class="overlay-card">
            <span class="spinner" aria-hidden="true"></span>
            <p>Conectando con el stream…</p>
          </div>
        </div>
      {:else if session.phase === "error" && session.origin !== "live"}
        <div class="overlay" role="alert">
          <div class="overlay-card error">
            <p class="overlay-title">Sin señal</p>
            <p class="overlay-text">
              {session.errorMessage ?? "La sesión se ha detenido."}
            </p>
          </div>
        </div>
      {/if}
    {:else}
      <div class="empty-state">
        <span class="empty-icon" aria-hidden="true">
          <svg
            width="28"
            height="28"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <rect x="2.5" y="4" width="19" height="13" rx="2" />
            <path d="M8 21h8M12 17v4" />
          </svg>
        </span>
        <h2>Sin sesión</h2>
        <p>Abre Dispositivos para ver el móvil conectado o iniciar el replay.</p>
      </div>
    {/if}
  </section>

  <div class="toolbar" role="toolbar" aria-label="Controles del dispositivo">
    <button class="tool-btn" type="button" title="Atrás" aria-label="Atrás" disabled={!session.inputEnabled} onclick={() => session.back()}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M19 12H5M11 6l-6 6 6 6" />
      </svg>
    </button>
    <button class="tool-btn" type="button" title="Home" aria-label="Home" disabled={!session.inputEnabled} onclick={() => session.home()}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
        <circle cx="12" cy="12" r="8.5" />
      </svg>
    </button>
    <button class="tool-btn" type="button" title="Recientes" aria-label="Recientes" disabled={!session.inputEnabled} onclick={() => session.recents()}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="5" y="5" width="14" height="14" rx="2" />
      </svg>
    </button>
    <span class="tool-sep" aria-hidden="true"></span>
    <button class="tool-btn" type="button" title="Rotar vertical/horizontal" aria-label="Rotar pantalla" disabled={!session.inputEnabled} onclick={() => session.rotate()}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M21 12a9 9 0 1 1-3-6.7" />
        <path d="M21 3v5h-5" />
        <rect x="9.5" y="9.5" width="5" height="5" rx="1" />
      </svg>
    </button>
    <span class="tool-sep" aria-hidden="true"></span>
    <button class="tool-btn" type="button" title="Copiar texto del móvil" aria-label="Copiar texto del móvil" disabled={!session.inputEnabled} onclick={() => void copyFromDevice()}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="8" y="8" width="12" height="12" rx="2" />
        <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
      </svg>
    </button>
    <button class="tool-btn" type="button" title="Pegar texto en el móvil" aria-label="Pegar texto en el móvil" disabled={!session.inputEnabled} onclick={() => void pasteToDevice()}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="5" y="4" width="14" height="17" rx="2" />
        <path d="M9 4V2h6v2M9 10h6M9 14h6" />
      </svg>
    </button>
  </div>

  {#if toast !== null}
    <div class="toast" role="status">{toast}</div>
  {/if}
</main>

<style>
  .mirror {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: radial-gradient(120% 76% at 50% 0%, #101c31 0%, #0a1220 68%, #080f1b 100%);
  }

  .session-topbar {
    height: 46px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    background: rgba(11, 18, 32, 0.62);
    border-bottom: 1px solid var(--border-soft);
    min-width: 0;
    overflow: hidden;
  }

  .live-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
    background: var(--lime);
    box-shadow: 0 0 6px rgba(163, 230, 53, 0.55);
  }

  .live-dot.idle {
    background: var(--dim);
    box-shadow: none;
  }

  .device-name {
    font-size: 16px;
    font-weight: 650;
    letter-spacing: 0.1px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    max-width: 96px;
    flex: 0 1 auto;
  }

  .pill {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 9px;
    border-radius: 999px;
    font-size: 16px;
    font-weight: 600;
    color: #cbd5e1;
    background: #0c1626;
    border: 1px solid var(--border);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .pill-usb {
    color: var(--lime);
    font-weight: 700;
    letter-spacing: 0.3px;
  }

  .pill-sep {
    color: #475569;
  }

  .mini-stat {
    flex: none;
    display: none;
    font-size: 16px;
    font-weight: 600;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .stop-btn {
    margin-left: auto;
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 10px;
    border-radius: 9px;
    font-size: 16px;
    font-weight: 650;
    color: #fecaca;
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.32);
    transition:
      background var(--transition),
      color var(--transition);
  }

  .stop-btn:hover {
    color: #fff;
    background: rgba(239, 68, 68, 0.24);
  }

  .stop-btn:active {
    transform: translateY(1px);
  }

  .toast {
    position: absolute;
    top: 58px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 40;
    max-width: calc(100% - 24px);
    padding: 8px 14px;
    border-radius: 999px;
    font-size: 16px;
    white-space: nowrap;
    color: var(--warn);
    background: rgba(8, 13, 23, 0.94);
    border: 1px solid rgba(245, 158, 11, 0.4);
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.4);
    pointer-events: none;
    animation: toast-in 150ms ease-out;
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translate(-50%, -6px);
    }
    to {
      opacity: 1;
      transform: translate(-50%, 0);
    }
  }

  .screen:focus-visible {
    outline: 2px solid var(--lime);
    outline-offset: 2px;
  }

  .error-slot {
    flex: none;
    padding: 10px 12px 0;
  }

  .stage {
    flex: 1;
    min-height: 0;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 12px 12px 58px;
    overflow: hidden;
  }

  .stage::before {
    content: "";
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: radial-gradient(58% 46% at 50% 48%, rgba(163, 230, 53, 0.045), transparent 72%);
  }

  .screen {
    position: relative;
    z-index: 1;
    display: block;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    border-radius: 12px;
    background: #04070d;
    border: 1px solid var(--border-soft);
  }

  .overlay {
    position: absolute;
    inset: 12px 12px 58px;
    z-index: 2;
    display: grid;
    place-items: center;
    padding: 16px;
    pointer-events: none;
  }

  .overlay-card {
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 260px;
    padding: 12px 16px;
    border-radius: 12px;
    font-size: 16px;
    color: var(--muted);
    background: rgba(8, 13, 23, 0.88);
    border: 1px solid var(--border);
    box-shadow: var(--shadow);
  }

  .overlay-card p {
    margin: 0;
    line-height: 1.4;
  }

  .overlay-card.error {
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    border-color: rgba(239, 68, 68, 0.35);
    color: #cbd5e1;
  }

  .overlay-title {
    font-weight: 650;
    color: #fca5a5;
  }

  .overlay-text {
    color: var(--muted);
  }

  .spinner {
    width: 16px;
    height: 16px;
    flex: none;
    border-radius: 50%;
    border: 2px solid rgba(163, 230, 53, 0.25);
    border-top-color: var(--lime);
    animation: spin 800ms linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .empty-state {
    position: relative;
    z-index: 1;
    max-width: 280px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 24px 20px;
    text-align: center;
    border-radius: 16px;
    background: rgba(16, 26, 44, 0.72);
    border: 1px dashed var(--border);
  }

  .empty-icon {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    margin-bottom: 2px;
    border-radius: 14px;
    color: var(--lime);
    background: var(--lime-soft);
    border: 1px solid var(--lime-line);
  }

  .empty-state h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 650;
  }

  .empty-state p {
    margin: 0;
    font-size: 16px;
    line-height: 1.45;
    color: var(--muted);
  }

  .toolbar {
    position: absolute;
    left: 50%;
    bottom: 8px;
    transform: translateX(-50%);
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    background: rgba(11, 18, 32, 0.94);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow:
      0 10px 24px rgba(0, 0, 0, 0.45),
      inset 0 1px 0 rgba(255, 255, 255, 0.03);
    backdrop-filter: blur(10px);
  }

  .tool-btn {
    width: 40px;
    height: 36px;
    display: grid;
    place-items: center;
    border-radius: 9px;
    color: var(--muted);
  }

  .tool-btn:disabled {
    opacity: 0.45;
  }

  .tool-sep {
    width: 1px;
    height: 20px;
    flex: none;
    background: var(--border);
    margin: 0 3px;
  }

  @container app (min-width: 480px) {
    .mini-stat {
      display: inline;
    }
  }

  @container app (min-width: 560px) {
    .device-name {
      max-width: 150px;
    }
  }
</style>
