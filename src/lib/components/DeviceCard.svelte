<script lang="ts">
  import type { DeviceDto, DeviceStateDto } from "../models/api/dto";

  interface Props {
    device: DeviceDto;
    onAuthHelp: () => void;
    onOpenMirror: (serial: string) => void;
  }

  let { device, onAuthHelp, onOpenMirror }: Props = $props();

  const STATE_LABELS: Record<DeviceStateDto, string> = {
    connected: "Conectado",
    unauthorized: "Sin autorizar",
    offline: "Desconectado",
  };

  const stateLabel = $derived(STATE_LABELS[device.state]);
  const resolutionLabel = $derived(
    device.resolution
      ? `${device.resolution.width}×${device.resolution.height}`
      : null,
  );
  const batteryLabel = $derived(
    device.batteryPercent !== null
      ? `Batería ${device.batteryPercent} %`
      : null,
  );
  const subtitle = $derived(
    device.state === "connected"
      ? "USB · listo para espejar"
      : device.state === "unauthorized"
        ? "USB · depuración pendiente"
        : "USB · sin conexión",
  );
</script>

<article
  class="device-card"
  class:ok={device.state === "connected"}
  class:unauth={device.state === "unauthorized"}
  class:offline={device.state === "offline"}
>
  <div class="device-head">
    <span class="device-avatar" aria-hidden="true">
      <svg
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.8"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <rect x="7" y="2.5" width="10" height="19" rx="2.5" />
        {#if device.state === "unauthorized"}
          <path d="M12 8v4M12 15.5h.01" />
        {:else}
          <path d="M11 18.5h2" />
        {/if}
      </svg>
    </span>

    <div class="device-id">
      <div class="name">
        <span class="device-name">{device.name}</span>
        <span class="status" class:ok={device.state === "connected"} class:warn={device.state === "unauthorized"} class:offline={device.state === "offline"}>
          <span class="dot" aria-hidden="true"></span>
          {stateLabel}
        </span>
      </div>
      <div class="device-sub">{subtitle}</div>
    </div>
  </div>

  {#if resolutionLabel || device.androidVersion || batteryLabel}
    <div class="meta">
      {#if device.androidVersion}
        <span class="chip">Android {device.androidVersion}</span>
      {/if}
      {#if resolutionLabel}
        <span class="chip">{resolutionLabel}</span>
      {/if}
      {#if batteryLabel}
        <span class="chip">{batteryLabel}</span>
      {/if}
    </div>
  {/if}

  {#if device.state === "connected"}
    <button
      class="btn primary full"
      type="button"
      onclick={() => onOpenMirror(device.serial)}
      aria-label={`Abrir espejo de ${device.name}`}
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <rect x="2.5" y="4" width="19" height="13" rx="2" />
        <path d="M8 21h8M12 17v4" />
      </svg>
      Abrir espejo
    </button>
  {:else if device.state === "unauthorized"}
    <button class="btn warn full" type="button" onclick={onAuthHelp}>
      Cómo autorizar
    </button>
  {:else}
    <p class="offline-note">Reconecta el cable USB y pulsa refrescar.</p>
  {/if}
</article>

<style>
  .device-card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    border-radius: 14px;
    background: linear-gradient(180deg, #18243a, #141e31);
    border: 1px solid var(--border);
    overflow: hidden;
  }

  .device-card.ok {
    border-color: rgba(163, 230, 53, 0.24);
  }

  .device-card.ok::before {
    content: "";
    position: absolute;
    left: 0;
    top: 16px;
    bottom: 16px;
    width: 2px;
    border-radius: 0 2px 2px 0;
    background: var(--lime);
    opacity: 0.7;
  }

  .device-card.unauth {
    border-left: 2px solid rgba(245, 158, 11, 0.6);
  }

  .device-card.offline {
    opacity: 0.85;
  }

  .device-head {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .device-avatar {
    width: 38px;
    height: 38px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 11px;
    color: var(--lime);
    background: var(--lime-soft);
    border: 1px solid var(--lime-line);
  }

  .device-card.unauth .device-avatar {
    color: var(--warn);
    background: var(--warn-soft);
    border-color: rgba(245, 158, 11, 0.3);
  }

  .device-card.offline .device-avatar {
    color: var(--muted);
    background: rgba(148, 163, 184, 0.1);
    border-color: var(--border);
  }

  .device-id {
    flex: 1;
    min-width: 0;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: 16px;
    font-weight: 650;
    line-height: 1.3;
  }

  .device-name {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 9px;
    border-radius: 999px;
    font-size: 16px;
    font-weight: 700;
    letter-spacing: 0.1px;
    white-space: nowrap;
    color: var(--muted);
    background: rgba(148, 163, 184, 0.1);
  }

  .status.ok {
    color: var(--lime);
    background: var(--lime-soft);
  }

  .status.warn {
    color: #fbbf24;
    background: var(--warn-soft);
  }

  .status.offline {
    color: var(--muted);
    background: rgba(148, 163, 184, 0.12);
  }

  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
  }

  .device-sub {
    font-size: 16px;
    color: var(--muted);
    line-height: 1.4;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    padding: 3px 9px;
    border-radius: 999px;
    font-size: 16px;
    color: #b6c2d4;
    background: #0d1727;
    border: 1px solid var(--border-soft);
    white-space: nowrap;
  }

  .device-card :global(.btn) {
    min-height: 38px;
  }

  .offline-note {
    margin: 0;
    font-size: 16px;
    line-height: 1.4;
    color: var(--dim);
  }
</style>
