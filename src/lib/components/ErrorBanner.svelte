<script lang="ts">
  import type { ErrorDto } from "../models/api/dto";
  import { ERROR_MESSAGES } from "./errorMessages";

  interface Props {
    error: ErrorDto;
    onRetry: () => void;
  }

  let { error, onRetry }: Props = $props();

  const message = $derived(ERROR_MESSAGES[error.code]);
</script>

<div class="banner" role="alert" title={error.message}>
  <svg
    width="18"
    height="18"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path d="M12 3L2.5 20h19L12 3z" />
    <path d="M12 10v4M12 17.2h.01" />
  </svg>
  <div class="banner-body">
    <p class="banner-message">{message}</p>
  </div>
  <button class="btn ghost small" type="button" onclick={onRetry}>
    Reintentar
  </button>
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--danger-soft);
    border: 1px solid rgba(239, 68, 68, 0.36);
    color: #fecaca;
  }

  .banner-body {
    flex: 1;
    min-width: 0;
  }

  .banner-message {
    margin: 0;
    font-size: 16px;
    line-height: 1.4;
    font-weight: 600;
  }

  .banner :global(.btn) {
    flex: none;
    min-height: 34px;
  }
</style>
