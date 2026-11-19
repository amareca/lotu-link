import { ApiError, type LotuApi } from "../models/api/api";
import {
  toErrorDto,
  type ErrorDto,
  type SettingsDto,
  type VideoSettingsDto,
} from "../models/api/dto";

const DEFAULTS: VideoSettingsDto = {
  maxSize: 0,
  maxFps: 60,
  bitRate: 8_000_000,
};

/**
 * M4 settings state. The draft is edited by the view; only `save()` talks to
 * the backend, so Cancel is a pure local reset.
 */
export class SettingsViewModel {
  saved = $state<VideoSettingsDto>({ ...DEFAULTS });
  draft = $state<VideoSettingsDto>({ ...DEFAULTS });
  restartRequired = $state(false);
  onboardingSeen = $state(false);
  notice = $state<ErrorDto | null>(null);
  loading = $state(false);

  readonly dirty = $derived(
    this.draft.maxSize !== this.saved.maxSize ||
      this.draft.maxFps !== this.saved.maxFps ||
      this.draft.bitRate !== this.saved.bitRate,
  );

  readonly #api: LotuApi;

  constructor(api: LotuApi) {
    this.#api = api;
  }

  async load(): Promise<void> {
    this.loading = true;
    try {
      this.#apply(await this.#api.getSettings());
      this.notice = null;
    } catch (cause) {
      this.notice = errorFrom(cause);
    } finally {
      this.loading = false;
    }
  }

  /** Saves the draft. Returns false on failure, leaving the draft editable. */
  async save(): Promise<boolean> {
    this.notice = null;
    try {
      this.#apply(await this.#api.saveSettings({ ...this.draft }));
      return true;
    } catch (cause) {
      this.notice = errorFrom(cause);
      return false;
    }
  }

  /** Marks the help as seen; only the backend persists the flag. */
  async markSeen(): Promise<void> {
    try {
      this.#apply(await this.#api.markOnboardingSeen());
    } catch (cause) {
      this.notice = errorFrom(cause);
    }
  }

  reset(): void {
    this.draft = { ...this.saved };
    this.notice = null;
  }

  setResolution(maxSize: number): void {
    this.draft.maxSize = maxSize;
  }

  setFps(maxFps: number): void {
    this.draft.maxFps = maxFps;
  }

  setBitrate(mbps: number): void {
    this.draft.bitRate = Math.round(mbps * 1_000_000);
  }

  #apply(snapshot: SettingsDto): void {
    this.saved = { ...snapshot.video };
    this.draft = { ...snapshot.video };
    this.restartRequired = snapshot.restartRequired;
    this.onboardingSeen = snapshot.onboardingSeen;
  }
}

function errorFrom(cause: unknown): ErrorDto {
  return cause instanceof ApiError ? cause.dto : toErrorDto(cause);
}
