/**
 * Pure input mapping for M2 (contract §4.1).
 *
 * The mapper speaks Android semantics (keycodes, meta-state bits, scroll
 * notches) but knows nothing about byte layouts, Tauri, the DOM or the
 * WebSocket. All geometry is expressed in CSS pixels: the device-pixel ratio
 * only affects the canvas backing store and never enters this module, so the
 * mapping is identical at DPR 1 and DPR 2.
 *
 * Coordinate spaces:
 * - canvas CSS space: what `pointerdown`/`pointermove` report.
 * - display space: `frame.displayWidth/displayHeight`, what the renderer paints
 *   (`computeLetterbox` uses these dimensions, including any SPS cropping).
 * - config space: the last WebSocket config, the space Android injects into and
 *   the one sent as `screen_size`.
 */

import { computeLetterbox } from "../media/canvasRenderer";

export interface Point {
  readonly x: number;
  readonly y: number;
}

export interface ScrollDelta {
  readonly hScroll: number;
  readonly vScroll: number;
}

/** DOM `WheelEvent.deltaMode` values used by M2. */
export type WheelDeltaMode = 0 | 1 | 2;

/** CSS pixels per wheel notch when `deltaMode` is `DOM_DELTA_PIXEL` (0). */
export const PX_PER_NOTCH = 100;

/** Lines per wheel notch when `deltaMode` is `DOM_DELTA_LINE` (1). */
export const LINES_PER_NOTCH = 3;

/** Pages per wheel notch when `deltaMode` is `DOM_DELTA_PAGE` (2). */
export const PAGES_PER_NOTCH = 1;

/** scrcpy clamps scroll to `[-16, 16]` notches. */
export const MAX_SCROLL_NOTCHES = 16;

/** Android `AMOTION_EVENT_BUTTON_PRIMARY`; the only button M2 emits. */
export const BUTTON_PRIMARY = 1;

/** Android `metaState` bits (`android/input.h`, contract §2.3). */
export const META_SHIFT_ON = 0x01;
export const META_ALT_ON = 0x02;
export const META_ALT_LEFT_ON = 0x10;
export const META_ALT_RIGHT_ON = 0x20;
export const META_SHIFT_LEFT_ON = 0x40;
export const META_SHIFT_RIGHT_ON = 0x80;
export const META_CTRL_ON = 0x1000;
export const META_CTRL_LEFT_ON = 0x2000;
export const META_CTRL_RIGHT_ON = 0x4000;
export const META_META_ON = 0x10000;
export const META_META_LEFT_ON = 0x20000;
export const META_META_RIGHT_ON = 0x40000;
export const META_CAPS_LOCK_ON = 0x100000;

/**
 * Android keycodes used by M2 (contract §2.3). Names mirror
 * `android/keycodes.h` (`del` is Backspace, `forwardDel` is the Delete key).
 */
export const KEYCODES = {
  home: 3,
  back: 4,
  dpadUp: 19,
  dpadDown: 20,
  dpadLeft: 21,
  dpadRight: 22,
  tab: 61,
  enter: 66,
  del: 67,
  pageUp: 92,
  pageDown: 93,
  escape: 111,
  forwardDel: 112,
  moveHome: 122,
  moveEnd: 123,
  appSwitch: 187,
} as const;

export type AndroidKeycode = (typeof KEYCODES)[keyof typeof KEYCODES];

/** `KeyboardEvent.location` values, used to pick the LEFT/RIGHT meta bits. */
export const MODIFIER_LOCATION = {
  STANDARD: 0,
  LEFT: 1,
  RIGHT: 2,
  NUMPAD: 3,
} as const;

/** DOM `KeyboardEvent.key` values mapped to Android keycodes. */
const SPECIAL_KEYCODES: Readonly<Record<string, AndroidKeycode>> = {
  Enter: KEYCODES.enter,
  Backspace: KEYCODES.del,
  Delete: KEYCODES.forwardDel,
  Tab: KEYCODES.tab,
  Escape: KEYCODES.escape,
  ArrowUp: KEYCODES.dpadUp,
  ArrowDown: KEYCODES.dpadDown,
  ArrowLeft: KEYCODES.dpadLeft,
  ArrowRight: KEYCODES.dpadRight,
  Home: KEYCODES.moveHome,
  End: KEYCODES.moveEnd,
  PageUp: KEYCODES.pageUp,
  PageDown: KEYCODES.pageDown,
};

/**
 * The subset of `KeyboardEvent` the mapper needs. `capsLock` and `altGraph`
 * come from `event.getModifierState("CapsLock")` / `("AltGraph")`; the DOM type
 * is not referenced here so the mapper stays runnable in node.
 */
export interface KeyEventInput {
  readonly key: string;
  readonly location: number;
  readonly shiftKey: boolean;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
  readonly isComposing: boolean;
  readonly capsLock: boolean;
  readonly altGraph: boolean;
}

export interface KeyMappingKey {
  readonly kind: "key";
  readonly keycode: AndroidKeycode;
  readonly metaState: number;
}

export interface KeyMappingText {
  readonly kind: "text";
  readonly text: string;
}

export type KeyMapping = KeyMappingKey | KeyMappingText;

/**
 * Maps a canvas CSS point to config-space device pixels.
 *
 * The letterbox is computed from the display dimensions (what the renderer
 * paints); black bars return `null`. Inside the content rectangle the point is
 * converted to display space and scaled to config space, then rounded and
 * clamped to `[0, config-1]` so the last pixel never overflows. The content
 * rectangle includes its right/bottom edges.
 */
export function mapPoint(
  cssX: number,
  cssY: number,
  canvasCssWidth: number,
  canvasCssHeight: number,
  displayWidth: number,
  displayHeight: number,
  configWidth: number,
  configHeight: number,
): Point | null {
  if (!Number.isFinite(cssX) || !Number.isFinite(cssY)) return null;
  if (!isPositive(configWidth) || !isPositive(configHeight)) return null;

  const rect = computeLetterbox(
    canvasCssWidth,
    canvasCssHeight,
    displayWidth,
    displayHeight,
  );
  if (rect.width <= 0 || rect.height <= 0) return null;

  const insideX = cssX >= rect.x && cssX <= rect.x + rect.width;
  const insideY = cssY >= rect.y && cssY <= rect.y + rect.height;
  if (!insideX || !insideY) return null;

  const xDisplay = ((cssX - rect.x) / rect.width) * displayWidth;
  const yDisplay = ((cssY - rect.y) / rect.height) * displayHeight;

  return {
    x: clamp(
      Math.round((xDisplay * configWidth) / displayWidth),
      0,
      configWidth - 1,
    ),
    y: clamp(
      Math.round((yDisplay * configHeight) / displayHeight),
      0,
      configHeight - 1,
    ),
  };
}

/**
 * Converts wheel deltas to scrcpy notches: `+hScroll` is right and `+vScroll`
 * is up, so the vertical sign is inverted. A zero result on both axes means
 * "nothing to send". Unknown delta modes fall back to pixels.
 */
export function wheelToScroll(
  deltaX: number,
  deltaY: number,
  deltaMode: WheelDeltaMode,
): ScrollDelta {
  const hNotches = clampScroll(deltaToNotches(deltaX, deltaMode));
  const vNotches = clampScroll(-deltaToNotches(deltaY, deltaMode));

  return {
    hScroll: normalizeZero(hNotches),
    vScroll: normalizeZero(vNotches),
  };
}

/**
 * Maps a key event to a keycode, injectable text or nothing.
 *
 * - Composition is ignored entirely (the commit arrives through
 *   `compositionend` → `sendText`).
 * - Special keys keep the modifiers in `metaState` (Shift+Tab, Ctrl+arrows).
 * - AltGr (Windows reports it as Ctrl+Alt, sometimes plus AltGraph) produces a
 *   printable character, so `key` is injected as text before the shortcut rule;
 *   otherwise "@", "€" and "#" would be dropped.
 * - Printable keys are injected as text only without Ctrl/Meta/Alt; the
 *   browser already applied Shift/CapsLock to `key` ("A", "á").
 * - Modifier keys, shortcuts and unmapped keys return `null` (D13).
 */
export function mapKey(input: KeyEventInput): KeyMapping | null {
  if (input.isComposing) return null;

  const special = SPECIAL_KEYCODES[input.key];
  if (special !== undefined) {
    return {
      kind: "key",
      keycode: special,
      metaState: modifierState(input),
    };
  }

  if (input.key.length !== 1) return null;
  if (input.altGraph) return { kind: "text", text: input.key };
  if (input.ctrlKey || input.metaKey || input.altKey) return null;

  return { kind: "text", text: input.key };
}

function modifierState(input: KeyEventInput): number {
  let state = 0;

  if (input.shiftKey) {
    state |= META_SHIFT_ON;
    state |= sideBits(
      input.location,
      META_SHIFT_LEFT_ON,
      META_SHIFT_RIGHT_ON,
    );
  }
  if (input.altKey) {
    state |= META_ALT_ON;
    state |= sideBits(input.location, META_ALT_LEFT_ON, META_ALT_RIGHT_ON);
  }
  if (input.ctrlKey) {
    state |= META_CTRL_ON;
    state |= sideBits(input.location, META_CTRL_LEFT_ON, META_CTRL_RIGHT_ON);
  }
  if (input.metaKey) {
    state |= META_META_ON;
    state |= sideBits(input.location, META_META_LEFT_ON, META_META_RIGHT_ON);
  }
  if (input.capsLock) state |= META_CAPS_LOCK_ON;

  return state;
}

/** Applies the event location to the pressed modifier's LEFT/RIGHT bits. */
function sideBits(location: number, left: number, right: number): number {
  if (location === MODIFIER_LOCATION.LEFT) return left;
  if (location === MODIFIER_LOCATION.RIGHT) return right;

  return 0;
}

function deltaToNotches(delta: number, deltaMode: WheelDeltaMode): number {
  if (!Number.isFinite(delta)) return 0;

  switch (deltaMode) {
    case 1:
      return delta / LINES_PER_NOTCH;
    case 2:
      return delta * PAGES_PER_NOTCH;
    default:
      return delta / PX_PER_NOTCH;
  }
}

function clampScroll(notches: number): number {
  if (!Number.isFinite(notches)) return 0;

  return clamp(notches, -MAX_SCROLL_NOTCHES, MAX_SCROLL_NOTCHES);
}

/** Avoids `-0` in payloads, which is not the same as `0` under `Object.is`. */
function normalizeZero(value: number): number {
  return value === 0 ? 0 : value;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function isPositive(value: number): boolean {
  return Number.isFinite(value) && value > 0;
}
