<script lang="ts">
  import { writeSystemClipboard } from "../models/systemClipboard";

  interface Props {
    onClose: () => void;
    fingerprint: string | null;
    error: string | null;
    loading: boolean;
  }

  let { onClose, fingerprint, error, loading }: Props = $props();

  let closeButton = $state<HTMLButtonElement | null>(null);
  let copied = $state(false);

  async function copyFingerprint(): Promise<void> {
    if (fingerprint === null) return;
    try {
      await writeSystemClipboard(fingerprint);
      copied = true;
    } catch {
      copied = false;
    }
  }

  $effect(() => {
    const frame = requestAnimationFrame(() => {
      closeButton?.focus({ preventScroll: true });
    });

    return () => cancelAnimationFrame(frame);
  });
</script>

<div class="modal-layer">
  <button
    class="modal-backdrop"
    type="button"
    aria-label="Cerrar autorización"
    onclick={onClose}
  ></button>

  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="auth-title">
    <header class="modal-head">
      <h2 id="auth-title">Autorizar depuración USB</h2>
      <button
        class="icon-btn"
        type="button"
        bind:this={closeButton}
        onclick={onClose}
        aria-label="Cerrar autorización"
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

    <div class="modal-body">
      <p class="modal-lede">
        Sigue estos pasos en el dispositivo. Al terminar, pulsa refrescar en Dispositivos.
      </p>

      <div class="auth-step">
        <span class="step-num" aria-hidden="true">1</span>
        <div>
          <div class="step-title">Activa las opciones de desarrollador</div>
          <div class="step-desc">
            Ajustes → Acerca del teléfono → Información de software → 7 toques en
            «Número de compilación».
          </div>
        </div>
      </div>

      <div class="auth-step">
        <span class="step-num" aria-hidden="true">2</span>
        <div>
          <div class="step-title">Activa la depuración USB</div>
          <div class="step-desc">
            Ajustes → Opciones de desarrollador → «Depuración USB». Reconecta el cable
            si hace falta.
          </div>
        </div>
      </div>

      <div class="auth-step">
        <span class="step-num" aria-hidden="true">3</span>
        <div>
          <div class="step-title">Acepta la autorización en el móvil</div>
          <div class="step-desc">
            Marca «Permitir siempre desde este equipo» y pulsa Permitir en el
            dispositivo.
          </div>
        </div>
      </div>

      <div class="auth-note">
        {#if loading}
          Leyendo la huella RSA de este equipo…
        {:else if fingerprint !== null}
          <div class="fingerprint-label">Huella RSA de este equipo</div>
          <div class="fingerprint-row">
            <code class="fingerprint">{fingerprint}</code>
            <button
              class="btn small"
              type="button"
              onclick={() => void copyFingerprint()}
              title="Copiar huella"
            >
              {copied ? "Copiada" : "Copiar"}
            </button>
          </div>
          <p class="fingerprint-help">Debe coincidir con la huella del diálogo del móvil.</p>
        {:else}
          <div class="fingerprint-label">No se pudo leer la huella RSA</div>
          <p class="fingerprint-help">
            {error ?? "Conecta el móvil y vuelve a intentarlo."}
          </p>
        {/if}
      </div>
    </div>

    <footer class="modal-foot">
      <button class="btn primary full" type="button" onclick={onClose}>
        Entendido
      </button>
    </footer>
  </div>
</div>

<style>
  .modal-layer {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: grid;
    place-items: center;
    padding: 16px;
  }

  .modal-backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    background: rgba(3, 7, 14, 0.74);
    backdrop-filter: blur(3px);
    cursor: default;
    animation: fade 150ms ease;
  }

  .modal {
    position: relative;
    z-index: 1;
    width: 100%;
    max-width: 380px;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: linear-gradient(180deg, #172238, #111a2a);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 26px 64px rgba(0, 0, 0, 0.62);
    animation: pop 160ms ease;
  }

  .modal-head {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 58px;
    padding: 0 12px 0 16px;
    border-bottom: 1px solid var(--border-soft);
  }

  .modal-head h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 18px;
    font-weight: 650;
  }

  .modal-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 14px 16px;
    scrollbar-width: thin;
    scrollbar-color: #2b3a52 transparent;
  }

  .modal-lede {
    margin: 0 0 10px;
    font-size: 16px;
    line-height: 1.45;
    color: var(--muted);
  }

  .auth-step {
    display: flex;
    gap: 10px;
    padding: 10px 0;
    border-top: 1px solid rgba(38, 52, 73, 0.5);
  }

  .auth-step:first-of-type {
    border-top: 0;
    padding-top: 0;
  }

  .step-num {
    width: 26px;
    height: 26px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    font-size: 16px;
    font-weight: 700;
    color: var(--lime);
    background: var(--lime-soft);
    border: 1px solid var(--lime-line);
  }

  .step-title {
    font-size: 16px;
    font-weight: 650;
    line-height: 1.3;
  }

  .step-desc {
    margin-top: 2px;
    font-size: 16px;
    line-height: 1.4;
    color: var(--muted);
  }

  .auth-note {
    margin-top: 12px;
    padding: 10px 12px;
    font-size: 16px;
    line-height: 1.5;
    color: var(--muted);
    background: #0b1422;
    border: 1px solid var(--border-soft);
    border-radius: 10px;
  }

  .fingerprint-label {
    font-weight: 700;
    color: var(--text);
  }

  .fingerprint-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }

  .fingerprint {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    font: 16px/1.4 ui-monospace, Consolas, "Cascadia Mono", monospace;
    color: var(--lime);
  }

  .fingerprint-help {
    margin: 6px 0 0;
    font-size: 16px;
    line-height: 1.45;
    color: var(--dim);
  }

  .modal-foot {
    flex: none;
    display: flex;
    gap: 10px;
    padding: 12px 14px;
    border-top: 1px solid var(--border-soft);
    background: rgba(11, 18, 32, 0.5);
  }

  .modal-foot .btn {
    flex: 1;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.975);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
</style>
