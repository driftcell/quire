import { describe, expect, test } from "bun:test";
import { isNativeImeKeyEvent, type NativeImeKeyEventLike } from "./ime";

function fakeKeyEvent(options: {
  key?: string;
  isComposing?: boolean;
  keyCode?: number;
}): NativeImeKeyEventLike {
  return {
    key: options.key ?? "Enter",
    nativeEvent: {
      isComposing: options.isComposing ?? false,
      keyCode: options.keyCode ?? 13,
    },
  };
}

describe("isNativeImeKeyEvent", () => {
  test("returns false for a plain Enter keydown outside IME composition", () => {
    expect(
      isNativeImeKeyEvent(
        fakeKeyEvent({ key: "Enter", isComposing: false, keyCode: 13 }),
      ),
    ).toBe(false);
  });

  test("returns true when nativeEvent.isComposing is true", () => {
    expect(
      isNativeImeKeyEvent(
        fakeKeyEvent({ key: "Enter", isComposing: true, keyCode: 13 }),
      ),
    ).toBe(true);
  });

  test("returns true when keyCode is 229 even if Safari already set isComposing to false", () => {
    expect(
      isNativeImeKeyEvent(
        fakeKeyEvent({ key: "Enter", isComposing: false, keyCode: 229 }),
      ),
    ).toBe(true);
  });

  test("returns true when key is Process", () => {
    expect(
      isNativeImeKeyEvent(
        fakeKeyEvent({ key: "Process", isComposing: false, keyCode: 13 }),
      ),
    ).toBe(true);
  });
});
