import { describe, expect, test } from "bun:test";
import {
  SmoothStreamClock,
  SmoothStreamPlayer,
  type SmoothStreamScheduler,
  type SmoothStreamState,
} from "./useSmoothStreamText";

class FakeScheduler implements SmoothStreamScheduler {
  time = 0;
  cancelled: number[] = [];
  private nextId = 1;
  private callbacks = new Map<number, (timestamp: number) => void>();

  now(): number {
    return this.time;
  }

  request(callback: (timestamp: number) => void): number {
    const id = this.nextId;
    this.nextId += 1;
    this.callbacks.set(id, callback);
    return id;
  }

  cancel(id: number): void {
    this.callbacks.delete(id);
    this.cancelled.push(id);
  }

  get pendingCount(): number {
    return this.callbacks.size;
  }

  step(deltaMs = 16): void {
    this.time += deltaMs;
    const callbacks = Array.from(this.callbacks.values());
    this.callbacks.clear();
    for (const callback of callbacks) {
      callback(this.time);
    }
  }
}

function runUntilIdle(scheduler: FakeScheduler, limit = 240): number {
  let frames = 0;
  while (scheduler.pendingCount > 0 && frames < limit) {
    scheduler.step();
    frames += 1;
  }
  return frames;
}

describe("useSmoothStreamText clock", () => {
  test("returns historical text immediately and never schedules a frame", () => {
    const scheduler = new FakeScheduler();
    let state: SmoothStreamState = { text: "", isSmoothing: false };
    const player = new SmoothStreamPlayer(
      "历史全文",
      false,
      (next) => {
        state = next;
      },
      scheduler,
    );
    expect(player.currentText).toBe("历史全文");
    const synced = player.sync("历史全文更新", false);
    expect(synced).toEqual({
      text: "历史全文更新",
      isSmoothing: false,
    });
    expect(state).toEqual(synced);
    expect(scheduler.pendingCount).toBe(0);
    player.stop();
  });

  test("seeds an already-streaming mount without replaying its text", () => {
    const clock = new SmoothStreamClock("切换标签页前的内容", true);
    expect(clock.currentText).toBe("切换标签页前的内容");
    expect(clock.needsFrame).toBe(false);
  });

  test("catches up a burst within the lag budget without splitting emoji", () => {
    const scheduler = new FakeScheduler();
    let state: SmoothStreamState = { text: "", isSmoothing: false };
    const player = new SmoothStreamPlayer(
      "",
      true,
      (next) => {
        state = next;
      },
      scheduler,
    );
    player.sync("", true);
    const target = `${"起始".repeat(20)}${"增长".repeat(800)}🙂中文🙃`;
    player.sync(target, true);

    const frames = runUntilIdle(scheduler);
    expect(state.text).toBe(target);
    expect(state.isSmoothing).toBe(false);
    expect(frames).toBeLessThan(60);
    expect(frames).toBeGreaterThan(0);
    player.stop();
  });

  test("bounds large subagent bursts to a short smooth tail", () => {
    const clock = new SmoothStreamClock("", true, 140, 80);
    const target = "前".repeat(900) + "🙂" + "尾".repeat(99);
    clock.sync(target, true);

    expect(Array.from(clock.currentText).length).toBe(920);
    expect(clock.currentText).toBe(Array.from(target).slice(0, 920).join(""));
    expect(clock.needsFrame).toBe(true);
  });

  test("advances only on Unicode code-point boundaries", () => {
    const target = "A🙂中文🙃B".repeat(40);
    const clock = new SmoothStreamClock("", true);
    clock.sync(target, true);
    let timestamp = 0;
    for (let frame = 0; frame < 120; frame += 1) {
      timestamp += 16;
      const result = clock.nextFrame(timestamp);
      expect(target.startsWith(result.text)).toBe(true);
      const displayedCodePoints = Array.from(result.text);
      expect(result.text).toBe(
        Array.from(target).slice(0, displayedCodePoints.length).join(""),
      );
      if (result.done) break;
    }
    expect(clock.currentText).toBe(target);
  });

  test("flushes the tail and stops rAF when streaming ends", () => {
    const scheduler = new FakeScheduler();
    let state: SmoothStreamState = { text: "", isSmoothing: false };
    const player = new SmoothStreamPlayer(
      "",
      true,
      (next) => {
        state = next;
      },
      scheduler,
    );
    const target = "收尾段落".repeat(300);
    player.sync(target, true);
    scheduler.step();
    expect(state.text.length).toBeLessThan(target.length);

    player.sync(target, false);
    const frames = runUntilIdle(scheduler);
    expect(frames).toBeGreaterThan(0);
    expect(state.text).toBe(target);
    expect(state.isSmoothing).toBe(false);
    expect(scheduler.pendingCount).toBe(0);

    player.sync(`${target}追加`, true);
    expect(scheduler.pendingCount).toBeGreaterThan(0);
    const cancelledBefore = scheduler.cancelled.length;
    player.stop();
    expect(scheduler.pendingCount).toBe(0);
    expect(scheduler.cancelled.length).toBeGreaterThan(cancelledBefore);
  });
});
