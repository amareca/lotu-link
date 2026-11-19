<script lang="ts">
  import { errorMessageForCode } from "../components/errorMessages";
  import type { SettingsViewModel } from "../viewmodels/SettingsViewModel.svelte";

  interface Props {
    settings: SettingsViewModel;
  }

  let { settings }: Props = $props();
</script>

<div class="settings-view">
  <div class="group-label">Vídeo</div>

  <div class="field">
    <div class="field-row">
      <span id="settings-resolution" class="field-label">Resolución</span>
      <span class="hint-inline">lado mayor</span>
    </div>
    <div
      class="segmented"
      role="radiogroup"
      aria-labelledby="settings-resolution"
    >
      <button
        type="button"
        class:on={settings.draft.maxSize === 0}
        role="radio"
        aria-checked={settings.draft.maxSize === 0}
        onclick={() => settings.setResolution(0)}
      >
        Nativa
      </button>
      <button
        type="button"
        class:on={settings.draft.maxSize === 1920}
        role="radio"
        aria-checked={settings.draft.maxSize === 1920}
        onclick={() => settings.setResolution(1920)}
      >
        1920
      </button>
      <button
        type="button"
        class:on={settings.draft.maxSize === 1280}
        role="radio"
        aria-checked={settings.draft.maxSize === 1280}
        onclick={() => settings.setResolution(1280)}
      >
        1280
      </button>
    </div>
  </div>

  <div class="field">
    <div class="field-row">
      <label for="settings-bitrate" class="field-label">Bitrate</label>
      <span class="value">{(settings.draft.bitRate / 1_000_000).toFixed(1)} Mbps</span>
    </div>
    <input
      id="settings-bitrate"
      type="range"
      min="2"
      max="20"
      step="0.5"
      value={settings.draft.bitRate / 1_000_000}
      aria-label="Bitrate en Mbps"
      oninput={(event) => settings.setBitrate(event.currentTarget.valueAsNumber)}
    />
  </div>

  <div class="field">
    <div class="field-row">
      <span id="settings-fps" class="field-label">FPS</span>
      <span class="hint-inline">máximos</span>
    </div>
    <div class="segmented" role="radiogroup" aria-labelledby="settings-fps">
      <button
        type="button"
        class:on={settings.draft.maxFps === 30}
        role="radio"
        aria-checked={settings.draft.maxFps === 30}
        onclick={() => settings.setFps(30)}
      >
        30
      </button>
      <button
        type="button"
        class:on={settings.draft.maxFps === 60}
        role="radio"
        aria-checked={settings.draft.maxFps === 60}
        onclick={() => settings.setFps(60)}
      >
        60
      </button>
    </div>
  </div>

  <div class="group-label">Archivos</div>

  <div class="field">
    <div class="field-row">
      <span class="field-label">Capturas</span>
    </div>
    <div class="folder">
      <span class="folder-path" title="Carpeta de capturas">Imágenes\LOTU Link</span>
      <button class="btn small" type="button" disabled>Cambiar</button>
    </div>
  </div>

  <div class="field">
    <div class="field-row">
      <span class="field-label">Grabaciones</span>
    </div>
    <div class="folder">
      <span class="folder-path" title="Carpeta de grabaciones">Vídeos\LOTU Link</span>
      <button class="btn small" type="button" disabled>Cambiar</button>
    </div>
  </div>

  <div class="group-label">ADB</div>

  <div class="field">
    <div class="radio-list">
      <label class="radio">
        <input type="radio" name="adb-source" value="bundled" checked disabled />
        Empaquetado
        <span class="chip">recomendado</span>
      </label>
      <label class="radio">
        <input type="radio" name="adb-source" value="system" disabled />
        Del sistema
      </label>
    </div>
  </div>

  {#if settings.notice !== null}
    <p class="error" role="status">{errorMessageForCode(settings.notice.code)}</p>
  {/if}
  <p class="note">
    Los cambios se guardan en este equipo. Con una sesión en curso se aplican al
    reconectar.
  </p>
</div>

<style>
  .settings-view {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .group-label {
    padding: 6px 2px 0;
    font-size: 16px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.7px;
    color: var(--dim);
  }

  .field {
    padding: 12px;
    border-radius: 12px;
    background: var(--panel-2);
    border: 1px solid var(--border-soft);
  }

  .field-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .field-label,
  .field-row label {
    flex: 1;
    min-width: 0;
    font-size: 16px;
    font-weight: 600;
  }

  .hint-inline {
    font-size: 16px;
    color: var(--dim);
    white-space: nowrap;
  }

  .value {
    font-size: 16px;
    font-weight: 700;
    color: var(--lime);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .segmented {
    display: flex;
    width: 100%;
    margin-top: 8px;
    border: 1px solid var(--border);
    border-radius: 10px;
    overflow: hidden;
    background: #0b1422;
  }

  .segmented button {
    flex: 1;
    min-width: 0;
    height: 42px;
    font-size: 16px;
    font-weight: 600;
    color: var(--muted);
  }

  .segmented button + button {
    border-left: 1px solid var(--border);
  }

  .segmented button.on {
    background: var(--lime-soft);
    color: var(--lime);
  }

  .segmented button:disabled {
    opacity: 0.8;
  }

  input[type="range"] {
    display: block;
    width: 100%;
    height: 26px;
    margin-top: 8px;
    accent-color: var(--lime);
    cursor: pointer;
  }

  input[type="range"]:disabled {
    cursor: default;
  }

  .folder {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }

  .folder-path {
    flex: 1;
    min-width: 0;
    padding: 8px 10px;
    border-radius: 8px;
    font: 16px/1.3 ui-monospace, Consolas, "Cascadia Mono", monospace;
    color: var(--muted);
    background: #0b1422;
    border: 1px solid var(--border-soft);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .radio-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .radio {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 8px 9px;
    border-radius: 8px;
    font-size: 16px;
    color: #cbd5e1;
  }

  .radio input {
    width: 16px;
    height: 16px;
    flex: none;
    accent-color: var(--lime);
  }

  .chip {
    margin-left: auto;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 16px;
    color: #b6c2d4;
    background: #0d1727;
    border: 1px solid var(--border-soft);
    white-space: nowrap;
  }

  .error {
    margin: 0;
    padding: 8px 10px;
    border-radius: 8px;
    font-size: 16px;
    color: var(--danger);
    background: var(--danger-soft);
  }

  .note {
    margin: 0;
    padding: 0 3px 4px;
    font-size: 16px;
    line-height: 1.45;
    color: var(--dim);
  }
</style>
