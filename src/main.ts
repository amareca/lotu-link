import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import type { LotuApi } from "./lib/models/api/api";
import { FakeApi } from "./lib/models/api/fakeApi";
import { TauriApi } from "./lib/models/api/tauriApi";
import { CanvasRenderer } from "./lib/models/media/canvasRenderer";
import { StreamClient } from "./lib/models/media/streamClient";
import { WebCodecsSink } from "./lib/models/media/webCodecsSink";
import { AppViewModel } from "./lib/viewmodels/AppViewModel.svelte";
import { DevicesViewModel } from "./lib/viewmodels/DevicesViewModel.svelte";
import { SettingsViewModel } from "./lib/viewmodels/SettingsViewModel.svelte";
import {
  SessionViewModel,
  type MediaFactory,
} from "./lib/viewmodels/SessionViewModel.svelte";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const api: LotuApi = isTauriRuntime()
  ? new TauriApi()
  : new FakeApi({ delayMs: 450 });

/** Composition root: the ViewModel receives media through its constructor. */
const media: MediaFactory = {
  createRenderer: () => new CanvasRenderer(),
  createSink: (handlers) => new WebCodecsSink(handlers),
  createStreamClient: (handlers) => new StreamClient(handlers),
};

const app = new AppViewModel();
const devices = new DevicesViewModel(api);
const session = new SessionViewModel(api, media);
const settings = new SettingsViewModel(api);

const target = document.getElementById("app");
if (target === null) {
  throw new Error("Missing #app mount point");
}

mount(App, {
  target,
  props: { app, devices, session, settings },
});
