import { invoke } from "@tauri-apps/api/core";
import { ApiError, type LotuApi } from "./api";
import {
  parseDeviceDtos,
  parseReplaySessionDto,
  parseSessionDto,
  toErrorDto,
  type DeviceDto,
  type KeyAction,
  type KeyInput,
  type PointerInput,
  type ReplaySessionDto,
  type ScrollInput,
  type SessionDto,
} from "./dto";

/**
 * The only frontend module allowed to import Tauri.
 * Commands keep their Rust names; DTOs are validated before reaching the ViewModels.
 */
export class TauriApi implements LotuApi {
  async listDevices(): Promise<DeviceDto[]> {
    try {
      const payload: unknown = await invoke<unknown>("list_devices");
      return parseDeviceDtos(payload);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async startReplay(): Promise<ReplaySessionDto> {
    try {
      const payload: unknown = await invoke<unknown>("start_replay");
      return parseReplaySessionDto(payload);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async startSession(serial: string): Promise<SessionDto> {
    try {
      const payload: unknown = await invoke<unknown>("start_session", { serial });
      return parseSessionDto(payload);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async stopSession(): Promise<void> {
    try {
      await invoke<unknown>("stop_session");
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async sendPointer(input: PointerInput): Promise<void> {
    await this.#invokeInput("send_pointer", { input });
  }

  async sendScroll(input: ScrollInput): Promise<void> {
    await this.#invokeInput("send_scroll", { input });
  }

  async sendKey(input: KeyInput): Promise<void> {
    await this.#invokeInput("send_key", { input });
  }

  async sendText(text: string): Promise<void> {
    await this.#invokeInput("send_text", { text });
  }

  async sendBack(action: KeyAction): Promise<void> {
    await this.#invokeInput("send_back", { action });
  }

  async rotate(): Promise<void> {
    await this.#invokeInput("rotate", {});
  }

  async pasteText(text: string): Promise<void> {
    await this.#invokeInput("paste_text", { text });
  }

  async copyText(): Promise<string> {
    try {
      return await invoke<string>("copy_text");
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async #invokeInput(command: string, args: Record<string, unknown>): Promise<void> {
    try {
      await invoke<unknown>(command, args);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }
}
