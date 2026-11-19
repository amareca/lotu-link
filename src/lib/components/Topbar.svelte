<script lang="ts">
  type ConnectionTone = "ok" | "warn" | "muted";

  interface Props {
    label: string;
    tone?: ConnectionTone;
  }

  let { label, tone = "muted" }: Props = $props();
</script>

<header class="titlebar">
  <div class="brand">
    <span class="brand-mark" aria-hidden="true">
      <svg
        width="15"
        height="15"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <rect x="7" y="2.5" width="10" height="19" rx="2.5" />
        <path d="M11 18.5h2" />
      </svg>
    </span>
    <span class="brand-name">LOTU Link</span>
    <span class="brand-tag">MVP</span>
  </div>

  <div class="titlebar-right">
    <span class="conn" class:ok={tone === "ok"} class:warn={tone === "warn"}>
      <span class="dot" aria-hidden="true"></span>
      <span class="conn-text">{label}</span>
    </span>
  </div>
</header>

<style>
  .titlebar {
    height: 46px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 0 12px;
    background: linear-gradient(180deg, #111c2f, #0b1220);
    border-bottom: 1px solid var(--border-soft);
    user-select: none;
    overflow: hidden;
    z-index: 20;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .brand-mark {
    width: 24px;
    height: 24px;
    border-radius: 8px;
    flex: none;
    display: grid;
    place-items: center;
    color: #0b1220;
    background: linear-gradient(150deg, var(--lime), #84cc16);
    box-shadow: 0 2px 10px rgba(163, 230, 53, 0.3);
  }

  .brand-name {
    font-size: 16px;
    font-weight: 650;
    letter-spacing: 0.1px;
    white-space: nowrap;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .brand-tag {
    font-size: 16px;
    font-weight: 700;
    letter-spacing: 0.5px;
    flex: none;
    color: var(--lime);
    background: var(--lime-soft);
    border: 1px solid rgba(163, 230, 53, 0.24);
    padding: 1px 6px;
    border-radius: 6px;
  }

  .titlebar-right {
    margin-left: auto;
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .conn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    max-width: 100%;
    padding: 0 10px;
    border-radius: 999px;
    font-size: 16px;
    font-weight: 600;
    color: #cbd5e1;
    background: rgba(15, 23, 42, 0.72);
    border: 1px solid var(--border);
    white-space: nowrap;
  }

  .conn-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
    background: var(--dim);
  }

  .conn.ok .dot {
    background: var(--lime);
    box-shadow:
      0 0 0 3px rgba(163, 230, 53, 0.1),
      0 0 8px rgba(163, 230, 53, 0.6);
  }

  .conn.warn .dot {
    background: var(--warn);
    box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.1);
  }

  @container app (max-width: 400px) {
    .brand-tag {
      display: none;
    }
  }
</style>
