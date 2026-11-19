import { ApiError, type LotuApi } from "../models/api/api";
import { toErrorDto, type ErrorDto } from "../models/api/dto";

/** State of the USB authorization help modal. */
export class OnboardingViewModel {
  fingerprint = $state<string | null>(null);
  error = $state<string | null>(null);
  loading = $state(false);

  readonly #api: LotuApi;

  constructor(api: LotuApi) {
    this.#api = api;
  }

  async load(): Promise<void> {
    this.loading = true;
    this.error = null;
    this.fingerprint = null;
    try {
      const result = await this.#api.getRsaFingerprint();
      this.fingerprint = result.fingerprint;
      this.error = result.error;
    } catch (cause) {
      const error: ErrorDto = cause instanceof ApiError ? cause.dto : toErrorDto(cause);
      this.error = error.message;
    } finally {
      this.loading = false;
    }
  }
}
