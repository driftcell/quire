import { describe, expect, test } from "bun:test";
import { hydrationCursor } from "./useSessionEventStream";

describe("hydrationCursor", () => {
  test("adopts the hydrated watermark without rewinding a live cursor", () => {
    expect(hydrationCursor(40, 42)).toBe(42);
    expect(hydrationCursor(42, 42)).toBe(42);
    expect(hydrationCursor(42, 40)).toBe(42);
  });
});
