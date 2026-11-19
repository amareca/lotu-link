import { ApiError, type LotuApi } from "../models/api/api";
import { toErrorDto, type DeviceDto, type ErrorDto } from "../models/api/dto";

/**
 * Device list state. Owns the request lifecycle explicitly (`start`/`dispose`);
 * never opens subscriptions or effects from the ViewModel.
 */
export class DevicesViewModel {
  devices = $state<DeviceDto[]>([]);
  loading = $state(false);
  error = $state<ErrorDto | null>(null);
  lastScanAt = $state<Date | null>(null);

  readonly #api: LotuApi;
  #pending: Promise<void> | null = null;
  #started = false;
  #disposed = false;

  constructor(api: LotuApi) {
    this.#api = api;
  }

  start(): void {
    if (this.#started) return;
    this.#started = true;
    void this.refresh();
  }

  /** Loads the device list. Concurrent calls share the in-flight request. */
  refresh(): Promise<void> {
    if (this.#pending !== null) return this.#pending;

    const pending = this.#load();
    this.#pending = pending;

    void pending.then(
      () => {
        if (this.#pending === pending) this.#pending = null;
      },
      () => {
        if (this.#pending === pending) this.#pending = null;
      },
    );

    return pending;
  }

  dispose(): void {
    this.#disposed = true;
    this.#started = false;
    this.#pending = null;
    this.loading = false;
  }

  async #load(): Promise<void> {
    if (this.#disposed) return;

    this.loading = true;
    this.error = null;

    try {
      const devices = await this.#api.listDevices();
      if (this.#disposed) return;

      this.devices = devices;
      this.lastScanAt = new Date();
    } catch (cause) {
      if (this.#disposed) return;

      this.error = cause instanceof ApiError ? cause.dto : toErrorDto(cause);
    } finally {
      if (!this.#disposed) {
        this.loading = false;
      }
    }
  }
}
