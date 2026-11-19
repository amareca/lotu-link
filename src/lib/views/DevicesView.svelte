<script lang="ts">
  import DeviceCard from "../components/DeviceCard.svelte";
  import ErrorBanner from "../components/ErrorBanner.svelte";
  import type { DevicesViewModel } from "../viewmodels/DevicesViewModel.svelte";

  interface Props {
    devices: DevicesViewModel;
    onAuthHelp: () => void;
    onOpenMirror: (serial: string) => void;
  }

  let { devices, onAuthHelp, onOpenMirror }: Props = $props();

  const scanLabel = $derived(
    devices.lastScanAt
      ? `Último escaneo: ${formatScanTime(devices.lastScanAt)}`
      : "Último escaneo: —",
  );

  const countLabel = $derived(
    `${devices.devices.length} ${devices.devices.length === 1 ? "equipo" : "equipos"}`,
  );

  function formatScanTime(date: Date): string {
    return date.toLocaleTimeString("es-ES", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }
</script>

<div class="devices-view">
  <div class="scan-row">
    <span class="scan-label">{scanLabel}</span>
    <button
      class="icon-btn"
      type="button"
      onclick={() => void devices.refresh()}
      disabled={devices.loading}
      aria-label="Refrescar dispositivos"
      title="Refrescar dispositivos"
      aria-busy={devices.loading}
    >
      <svg
        class:spin={devices.loading}
        width="17"
        height="17"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M21 12a9 9 0 1 1-2.64-6.36" />
        <path d="M21 3v6h-6" />
      </svg>
    </button>
  </div>

  <div class="scan-meta">
    <span class="dot" aria-hidden="true"></span>
    {countLabel}
  </div>

  {#if devices.error}
    <ErrorBanner error={devices.error} onRetry={() => void devices.refresh()} />
  {/if}

  {#if devices.loading && devices.devices.length === 0}
    <p class="state-note">Buscando dispositivos por USB…</p>
  {:else if devices.devices.length === 0 && !devices.error}
    <div class="empty-state">
      <span class="empty-icon" aria-hidden="true">
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <rect x="7" y="2.5" width="10" height="19" rx="2.5" />
          <path d="M11 18.5h2" />
        </svg>
      </span>
      <p class="empty-title">No se detectan dispositivos</p>
      <p class="empty-text">
        Conecta un móvil por USB, activa la depuración USB y pulsa Refrescar.
      </p>
    </div>
  {:else}
    {#each devices.devices as device (device.serial)}
      <DeviceCard {device} {onAuthHelp} {onOpenMirror} />
    {/each}
  {/if}
</div>

<style>
  .devices-view {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .scan-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 0 2px;
  }

  .scan-label {
    min-width: 0;
    font-size: 16px;
    color: var(--dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-variant-numeric: tabular-nums;
  }

  .scan-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: -6px;
    padding: 0 2px;
    font-size: 16px;
    color: var(--muted);
  }

  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--lime);
  }

  .state-note {
    margin: 0;
    padding: 14px;
    border-radius: 12px;
    font-size: 16px;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--border-soft);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 26px 18px;
    text-align: center;
    border-radius: 14px;
    background: var(--panel-2);
    border: 1px dashed var(--border);
  }

  .empty-icon {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    margin-bottom: 4px;
    border-radius: 14px;
    color: var(--muted);
    background: rgba(148, 163, 184, 0.08);
    border: 1px solid var(--border-soft);
  }

  .empty-title {
    margin: 0;
    font-size: 16px;
    font-weight: 650;
  }

  .empty-text {
    margin: 0;
    font-size: 16px;
    line-height: 1.45;
    color: var(--muted);
  }
</style>
