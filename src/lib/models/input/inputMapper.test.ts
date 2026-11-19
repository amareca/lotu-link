import { afterEach, describe, expect, it } from "vitest";
import {
  BUTTON_PRIMARY,
  KEYCODES,
  LINES_PER_NOTCH,
  MAX_SCROLL_NOTCHES,
  META_ALT_LEFT_ON,
  META_ALT_ON,
  META_ALT_RIGHT_ON,
  META_CAPS_LOCK_ON,
  META_CTRL_LEFT_ON,
  META_CTRL_ON,
  META_CTRL_RIGHT_ON,
  META_META_LEFT_ON,
  META_META_ON,
  META_META_RIGHT_ON,
  META_SHIFT_LEFT_ON,
  META_SHIFT_ON,
  META_SHIFT_RIGHT_ON,
  MODIFIER_LOCATION,
  PX_PER_NOTCH,
  mapKey,
  mapPoint,
  wheelToScroll,
  type AndroidKeycode,
  type KeyEventInput,
  type KeyMapping,
  type WheelDeltaMode,
} from "./inputMapper";

const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");

afterEach(() => {
  if (originalWindow === undefined) {
    Reflect.deleteProperty(globalThis, "window");
  } else {
    Object.defineProperty(globalThis, "window", originalWindow);
  }
});

function setDevicePixelRatio(value: number): void {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    writable: true,
    value: { devicePixelRatio: value },
  });
}

describe("mapPoint", () => {
  // Portrait stream (1080x2400) in an 800x600 canvas: pillarbox with bars
  // 265px wide and content from x=265 to x=535.
  it("returns null on the side bars of a pillarboxed stream", () => {
    expect(mapPoint(0, 300, 800, 600, 1080, 2400, 1080, 2400)).toBeNull();
    expect(mapPoint(264, 300, 800, 600, 1080, 2400, 1080, 2400)).toBeNull();
    expect(mapPoint(536, 300, 800, 600, 1080, 2400, 1080, 2400)).toBeNull();
    expect(mapPoint(799, 300, 800, 600, 1080, 2400, 1080, 2400)).toBeNull();
  });

  it("returns null on the top and bottom bars of a letterboxed stream", () => {
    // Landscape stream (2400x1080) in an 800x600 canvas: content y=120..480.
    expect(mapPoint(400, 0, 800, 600, 2400, 1080, 2400, 1080)).toBeNull();
    expect(mapPoint(400, 119, 800, 600, 2400, 1080, 2400, 1080)).toBeNull();
    expect(mapPoint(400, 481, 800, 600, 2400, 1080, 2400, 1080)).toBeNull();
    expect(mapPoint(400, 599, 800, 600, 2400, 1080, 2400, 1080)).toBeNull();
  });

  it("maps the content edges and the center of a portrait stream", () => {
    expect(mapPoint(265, 0, 800, 600, 1080, 2400, 1080, 2400)).toEqual({
      x: 0,
      y: 0,
    });
    expect(mapPoint(535, 600, 800, 600, 1080, 2400, 1080, 2400)).toEqual({
      x: 1079,
      y: 2399,
    });
    expect(mapPoint(400, 300, 800, 600, 1080, 2400, 1080, 2400)).toEqual({
      x: 540,
      y: 1200,
    });
    expect(mapPoint(400, 120, 800, 600, 2400, 1080, 2400, 1080)).toEqual({
      x: 1200,
      y: 0,
    });
    expect(mapPoint(400, 480, 800, 600, 2400, 1080, 2400, 1080)).toEqual({
      x: 1200,
      y: 1079,
    });
  });

  it("scales display space to config space when the stream is cropped", () => {
    // Display 1200x2400 is cropped to the config 1080x2400 (SPS cropping).
    expect(mapPoint(200, 400, 400, 800, 1200, 2400, 1080, 2400)).toEqual({
      x: 540,
      y: 1200,
    });
    expect(mapPoint(0, 0, 400, 800, 1200, 2400, 1080, 2400)).toEqual({
      x: 0,
      y: 0,
    });
    expect(mapPoint(400, 800, 400, 800, 1200, 2400, 1080, 2400)).toEqual({
      x: 1079,
      y: 2399,
    });
  });

  it("rounds to the nearest config pixel", () => {
    // x: 540 * 1080 / 1080 = 540. y: 300 * 2340 / 2400 = 292.5 -> 293.
    expect(mapPoint(400, 75, 800, 600, 1080, 2400, 1080, 2340)).toEqual({
      x: 540,
      y: 293,
    });
  });

  it("clamps to the last config pixel on the content boundary", () => {
    expect(mapPoint(535, 600, 800, 600, 1080, 2400, 1080, 2400)).toEqual({
      x: 1079,
      y: 2399,
    });
    expect(mapPoint(400, 300, 800, 600, 1080, 2400, 1, 1)).toEqual({
      x: 0,
      y: 0,
    });
  });

  it("does not depend on the device pixel ratio", () => {
    const samples: ReadonlyArray<readonly [number, number]> = [
      [400, 300],
      [265, 0],
      [535, 600],
      [400, 120],
    ];

    setDevicePixelRatio(1);
    const atDpr1 = samples.map(([cssX, cssY]) =>
      mapPoint(cssX, cssY, 800, 600, 2400, 1080, 2400, 1080),
    );

    setDevicePixelRatio(2);
    const atDpr2 = samples.map(([cssX, cssY]) =>
      mapPoint(cssX, cssY, 800, 600, 2400, 1080, 2400, 1080),
    );

    expect(atDpr2).toEqual(atDpr1);
    expect(atDpr1[0]).toEqual({ x: 1200, y: 540 });
  });

  it("returns null for degenerate geometry and non-finite points", () => {
    expect(mapPoint(10, 10, 0, 600, 1080, 2400, 1080, 2400)).toBeNull();
    expect(mapPoint(10, 10, 800, 600, 0, 2400, 1080, 2400)).toBeNull();
    expect(mapPoint(10, 10, 800, 600, 1080, 2400, 0, 2400)).toBeNull();
    expect(mapPoint(10, 10, 800, 600, 1080, 2400, 1080, -1)).toBeNull();
    expect(
      mapPoint(Number.NaN, 10, 800, 600, 1080, 2400, 1080, 2400),
    ).toBeNull();
    expect(
      mapPoint(10, Number.POSITIVE_INFINITY, 800, 600, 1080, 2400, 1080, 2400),
    ).toBeNull();
  });
});

describe("wheelToScroll", () => {
  it("normalizes pixel deltas by PX_PER_NOTCH", () => {
    expect(PX_PER_NOTCH).toBe(100);
    expect(wheelToScroll(100, 100, 0)).toEqual({ hScroll: 1, vScroll: -1 });
    expect(wheelToScroll(-250, -250, 0)).toEqual({
      hScroll: -2.5,
      vScroll: 2.5,
    });
  });

  it("normalizes line deltas by LINES_PER_NOTCH", () => {
    expect(LINES_PER_NOTCH).toBe(3);
    expect(wheelToScroll(3, 3, 1)).toEqual({ hScroll: 1, vScroll: -1 });
    expect(wheelToScroll(1.5, -6, 1)).toEqual({ hScroll: 0.5, vScroll: 2 });
  });

  it("treats page deltas as one notch each", () => {
    expect(wheelToScroll(1, 1, 2)).toEqual({ hScroll: 1, vScroll: -1 });
    expect(wheelToScroll(-2, 3, 2)).toEqual({ hScroll: -2, vScroll: -3 });
  });

  it("clamps both axes to ±MAX_SCROLL_NOTCHES", () => {
    expect(MAX_SCROLL_NOTCHES).toBe(16);
    expect(wheelToScroll(1_600, -1_600, 0)).toEqual({
      hScroll: 16,
      vScroll: 16,
    });
    expect(wheelToScroll(1_000_000, -1_000_000, 0)).toEqual({
      hScroll: 16,
      vScroll: 16,
    });
    expect(wheelToScroll(-1_000_000, 1_000_000, 0)).toEqual({
      hScroll: -16,
      vScroll: -16,
    });
    expect(wheelToScroll(-100, 100, 2)).toEqual({ hScroll: -16, vScroll: -16 });
  });

  it("returns positive zeros when there is nothing to scroll", () => {
    const delta = wheelToScroll(0, 0, 0);

    expect(delta).toEqual({ hScroll: 0, vScroll: 0 });
    expect(Object.is(delta.hScroll, 0)).toBe(true);
    expect(Object.is(delta.vScroll, 0)).toBe(true);
  });

  it("ignores non-finite deltas and unknown delta modes", () => {
    expect(wheelToScroll(Number.NaN, Number.POSITIVE_INFINITY, 0)).toEqual({
      hScroll: 0,
      vScroll: 0,
    });
    // Browsers only define 0..2; anything else falls back to pixel units.
    expect(
      wheelToScroll(100, 100, 7 as unknown as WheelDeltaMode),
    ).toEqual({ hScroll: 1, vScroll: -1 });
  });
});

function keyEvent(overrides: Partial<KeyEventInput> = {}): KeyEventInput {
  return {
    key: "a",
    location: MODIFIER_LOCATION.STANDARD,
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    isComposing: false,
    capsLock: false,
    altGraph: false,
    ...overrides,
  };
}

function expectKey(
  mapping: KeyMapping | null,
  keycode: AndroidKeycode,
  metaState = 0,
): void {
  expect(mapping).toEqual({ kind: "key", keycode, metaState });
}

describe("mapKey", () => {
  it("maps the special keys of the contract", () => {
    const specials: ReadonlyArray<readonly [string, AndroidKeycode]> = [
      ["Enter", KEYCODES.enter],
      ["Backspace", KEYCODES.del],
      ["Delete", KEYCODES.forwardDel],
      ["Tab", KEYCODES.tab],
      ["Escape", KEYCODES.escape],
      ["ArrowUp", KEYCODES.dpadUp],
      ["ArrowDown", KEYCODES.dpadDown],
      ["ArrowLeft", KEYCODES.dpadLeft],
      ["ArrowRight", KEYCODES.dpadRight],
      ["Home", KEYCODES.moveHome],
      ["End", KEYCODES.moveEnd],
      ["PageUp", KEYCODES.pageUp],
      ["PageDown", KEYCODES.pageDown],
    ];

    for (const [key, keycode] of specials) {
      expectKey(mapKey(keyEvent({ key })), keycode);
    }
  });

  it("injects printable keys as text, keeping Shift and CapsLock casing", () => {
    expect(mapKey(keyEvent({ key: "a" }))).toEqual({ kind: "text", text: "a" });
    expect(mapKey(keyEvent({ key: "A", shiftKey: true }))).toEqual({
      kind: "text",
      text: "A",
    });
    expect(mapKey(keyEvent({ key: "á" }))).toEqual({
      kind: "text",
      text: "á",
    });
    expect(mapKey(keyEvent({ key: "ñ", capsLock: true }))).toEqual({
      kind: "text",
      text: "ñ",
    });
    expect(mapKey(keyEvent({ key: " " }))).toEqual({
      kind: "text",
      text: " ",
    });
  });

  it("drops printable keys combined with Ctrl, Meta or Alt", () => {
    expect(mapKey(keyEvent({ key: "c", ctrlKey: true }))).toBeNull();
    expect(mapKey(keyEvent({ key: "v", metaKey: true }))).toBeNull();
    expect(mapKey(keyEvent({ key: "x", altKey: true }))).toBeNull();
    expect(
      mapKey(keyEvent({ key: "c", ctrlKey: true, shiftKey: true })),
    ).toBeNull();
  });

  it("injects AltGr characters even when Windows reports Ctrl and Alt", () => {
    // Windows surfaces AltGr as Ctrl+Alt (plus AltGraph on some layouts).
    for (const key of ["@", "€", "#"]) {
      expect(
        mapKey(
          keyEvent({ key, altGraph: true, ctrlKey: true, altKey: true }),
        ),
      ).toEqual({ kind: "text", text: key });
    }
    expect(mapKey(keyEvent({ key: "€", altGraph: true }))).toEqual({
      kind: "text",
      text: "€",
    });
  });

  it("drops the same characters without AltGr", () => {
    for (const key of ["@", "€", "#"]) {
      expect(
        mapKey(keyEvent({ key, ctrlKey: true, altKey: true })),
      ).toBeNull();
    }
  });

  it("keeps special keys as keycodes with Ctrl, with or without AltGr", () => {
    expectKey(
      mapKey(keyEvent({ key: "Tab", ctrlKey: true, altGraph: true })),
      KEYCODES.tab,
      META_CTRL_ON,
    );
    expectKey(
      mapKey(
        keyEvent({
          key: "ArrowLeft",
          ctrlKey: true,
          location: MODIFIER_LOCATION.LEFT,
        }),
      ),
      KEYCODES.dpadLeft,
      META_CTRL_ON | META_CTRL_LEFT_ON,
    );
  });

  it("returns null for bare modifiers and unmapped keys", () => {
    for (const key of [
      "Shift",
      "Control",
      "Alt",
      "Meta",
      "CapsLock",
      "AltGraph",
      "F5",
      "Dead",
      "Unidentified",
    ]) {
      expect(mapKey(keyEvent({ key }))).toBeNull();
    }
  });

  it("ignores composing events", () => {
    expect(mapKey(keyEvent({ key: "a", isComposing: true }))).toBeNull();
    expect(mapKey(keyEvent({ key: "Enter", isComposing: true }))).toBeNull();
    expect(mapKey(keyEvent({ key: "ArrowLeft", isComposing: true }))).toBeNull();
  });

  it("encodes the Shift bits with the event location", () => {
    expectKey(
      mapKey(
        keyEvent({
          key: "Tab",
          shiftKey: true,
          location: MODIFIER_LOCATION.LEFT,
        }),
      ),
      KEYCODES.tab,
      META_SHIFT_ON | META_SHIFT_LEFT_ON,
    );
    expectKey(
      mapKey(
        keyEvent({
          key: "Tab",
          shiftKey: true,
          location: MODIFIER_LOCATION.RIGHT,
        }),
      ),
      KEYCODES.tab,
      META_SHIFT_ON | META_SHIFT_RIGHT_ON,
    );
    expectKey(
      mapKey(keyEvent({ key: "Tab", shiftKey: true })),
      KEYCODES.tab,
      META_SHIFT_ON,
    );
  });

  it("encodes the Ctrl bits with the event location", () => {
    expectKey(
      mapKey(
        keyEvent({
          key: "ArrowLeft",
          ctrlKey: true,
          location: MODIFIER_LOCATION.LEFT,
        }),
      ),
      KEYCODES.dpadLeft,
      META_CTRL_ON | META_CTRL_LEFT_ON,
    );
    expectKey(
      mapKey(
        keyEvent({
          key: "ArrowRight",
          ctrlKey: true,
          location: MODIFIER_LOCATION.RIGHT,
        }),
      ),
      KEYCODES.dpadRight,
      META_CTRL_ON | META_CTRL_RIGHT_ON,
    );
  });

  it("encodes the Alt bits with the event location", () => {
    expectKey(
      mapKey(
        keyEvent({
          key: "ArrowUp",
          altKey: true,
          location: MODIFIER_LOCATION.LEFT,
        }),
      ),
      KEYCODES.dpadUp,
      META_ALT_ON | META_ALT_LEFT_ON,
    );
    expectKey(
      mapKey(
        keyEvent({
          key: "ArrowDown",
          altKey: true,
          location: MODIFIER_LOCATION.RIGHT,
        }),
      ),
      KEYCODES.dpadDown,
      META_ALT_ON | META_ALT_RIGHT_ON,
    );
  });

  it("encodes the Meta bits with the event location", () => {
    expectKey(
      mapKey(
        keyEvent({
          key: "Home",
          metaKey: true,
          location: MODIFIER_LOCATION.LEFT,
        }),
      ),
      KEYCODES.moveHome,
      META_META_ON | META_META_LEFT_ON,
    );
    expectKey(
      mapKey(
        keyEvent({
          key: "End",
          metaKey: true,
          location: MODIFIER_LOCATION.RIGHT,
        }),
      ),
      KEYCODES.moveEnd,
      META_META_ON | META_META_RIGHT_ON,
    );
  });

  it("adds the CapsLock bit from the caller state", () => {
    expectKey(
      mapKey(keyEvent({ key: "Home", capsLock: true })),
      KEYCODES.moveHome,
      META_CAPS_LOCK_ON,
    );
    expectKey(
      mapKey(keyEvent({ key: "Enter", capsLock: true })),
      KEYCODES.enter,
      META_CAPS_LOCK_ON,
    );
  });

  it("does not add LEFT/RIGHT bits for standard or numpad locations", () => {
    expectKey(
      mapKey(
        keyEvent({
          key: "Enter",
          shiftKey: true,
          location: MODIFIER_LOCATION.NUMPAD,
        }),
      ),
      KEYCODES.enter,
      META_SHIFT_ON,
    );
  });

  it("combines all modifiers and the CapsLock bit", () => {
    expectKey(
      mapKey(
        keyEvent({
          key: "Enter",
          shiftKey: true,
          ctrlKey: true,
          altKey: true,
          metaKey: true,
          capsLock: true,
          location: MODIFIER_LOCATION.LEFT,
        }),
      ),
      KEYCODES.enter,
      META_SHIFT_ON |
        META_SHIFT_LEFT_ON |
        META_CTRL_ON |
        META_CTRL_LEFT_ON |
        META_ALT_ON |
        META_ALT_LEFT_ON |
        META_META_ON |
        META_META_LEFT_ON |
        META_CAPS_LOCK_ON,
    );
  });
});

describe("constants", () => {
  it("emits only the primary Android button", () => {
    expect(BUTTON_PRIMARY).toBe(1);
  });
});
