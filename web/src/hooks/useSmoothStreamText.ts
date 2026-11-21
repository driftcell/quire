import { useEffect, useState } from "react";

export const SMOOTH_STREAM_BASE_RATE = 140;
export const SMOOTH_STREAM_MAX_LAG_SECONDS = 0.35;
export const SMOOTH_STREAM_FINISH_SECONDS = 0.06;

export interface SmoothStreamState {
  text: string;
  isSmoothing: boolean;
}

export interface SmoothStreamScheduler {
  now(): number;
  request(callback: (timestamp: number) => void): number;
  cancel(id: number): void;
}

const browserScheduler: SmoothStreamScheduler = {
  now: () => performance.now(),
  request: (callback) => window.requestAnimationFrame(callback),
  cancel: (id) => window.cancelAnimationFrame(id),
};

function countCodePoints(text: string): number {
  let count = 0;
  for (let index = 0; index < text.length; count += 1) {
    const codePoint = text.codePointAt(index);
    index += codePoint !== undefined && codePoint > 0xffff ? 2 : 1;
  }
  return count;
}

/**
 * Pure, scheduler-free text clock. The player below owns rAF; tests can drive
 * this class directly without React or timers.
 */
export class SmoothStreamClock {
  private targetText: string;
  private targetUnits: number;
  private displayedText: string;
  private displayedIndex: number;
  private displayedUnits: number;
  private carry = 0;
  private lastFrameAt: number | null = null;
  private isStreaming: boolean;
  private wasStreaming: boolean;
  private finishing = false;
  private readonly baseRate: number;
  private readonly maxBacklog: number;

  constructor(
    targetText: string,
    isStreaming: boolean,
    baseRate = SMOOTH_STREAM_BASE_RATE,
    maxBacklog = Number.POSITIVE_INFINITY,
  ) {
    this.baseRate = baseRate;
    this.maxBacklog = maxBacklog;
    this.targetText = targetText;
    this.targetUnits = countCodePoints(targetText);
    // Initial streaming mount must not replay text already on screen.
    this.displayedText = targetText;
    this.displayedIndex = targetText.length;
    this.displayedUnits = this.targetUnits;
    this.isStreaming = isStreaming;
    this.wasStreaming = isStreaming;
  }

  get currentText(): string {
    return this.displayedText;
  }

  get currentIsSmoothing(): boolean {
    return this.displayedUnits < this.targetUnits;
  }

  get needsFrame(): boolean {
    return this.currentIsSmoothing;
  }

  /** Start a new rAF burst without inheriting an old timestamp gap. */
  beginFrame(): void {
    this.lastFrameAt = null;
  }

  /**
   * Keep only a bounded tail for subagent/history bursts. The existing prefix
   * is already settled content, not a blank region that should be typed again.
   */
  private boundBacklog(): void {
    const backlog = this.targetUnits - this.displayedUnits;
    if (backlog <= this.maxBacklog) {
      return;
    }
    const visibleUnits = this.targetUnits - this.maxBacklog;
    let index = 0;
    let units = 0;
    while (units < visibleUnits && index < this.targetText.length) {
      const codePoint = this.targetText.codePointAt(index);
      index += codePoint !== undefined && codePoint > 0xffff ? 2 : 1;
      units += 1;
    }
    this.displayedText = this.targetText.slice(0, index);
    this.displayedIndex = index;
    this.displayedUnits = units;
    this.carry = 0;
  }

  /**
   * Reconcile a new upstream target. Append-only growth is measured only over
   * the added suffix. A reset/replacement synchronizes immediately instead of
   * replaying unrelated content.
   */
  sync(targetText: string, isStreaming: boolean): SmoothStreamState {
    if (targetText !== this.targetText) {
      if (!targetText.startsWith(this.displayedText)) {
        this.displayedText = targetText;
        this.displayedIndex = targetText.length;
        this.displayedUnits = countCodePoints(targetText);
        this.targetUnits = this.displayedUnits;
        this.carry = 0;
        this.lastFrameAt = null;
      } else if (targetText.startsWith(this.targetText)) {
        this.targetUnits += countCodePoints(
          targetText.slice(this.targetText.length),
        );
      } else {
        this.targetUnits =
          this.displayedUnits +
          countCodePoints(targetText.slice(this.displayedIndex));
      }
      this.targetText = targetText;
    }

    if (isStreaming) {
      this.isStreaming = true;
      this.wasStreaming = true;
      this.finishing = false;
      this.boundBacklog();
    } else if (this.wasStreaming) {
      this.isStreaming = false;
      this.finishing = true;
    } else {
      this.isStreaming = false;
      this.finishing = false;
      // Historical/static replacements appear immediately.
      this.displayedText = this.targetText;
      this.displayedIndex = this.targetText.length;
      this.displayedUnits = this.targetUnits;
      this.carry = 0;
    }

    if (this.finishing && this.displayedUnits >= this.targetUnits) {
      this.finishing = false;
      this.wasStreaming = false;
    }

    return {
      text: this.displayedText,
      isSmoothing:
        this.currentIsSmoothing && (this.isStreaming || this.finishing),
    };
  }

  nextFrame(timestamp: number): SmoothStreamState & { done: boolean } {
    const backlog = this.targetUnits - this.displayedUnits;
    if (backlog <= 0) {
      if (!this.isStreaming) {
        this.finishing = false;
        this.wasStreaming = false;
      }
      this.lastFrameAt = null;
      return { text: this.displayedText, isSmoothing: false, done: true };
    }

    const deltaMs =
      this.lastFrameAt === null
        ? 16
        : Math.max(1, Math.min(timestamp - this.lastFrameAt, 1_000));
    this.lastFrameAt = timestamp;

    const lagSeconds = this.finishing
      ? SMOOTH_STREAM_FINISH_SECONDS
      : // 1/3 of the 350ms budget is the exponential time constant; after
        // roughly 3τ the backlog is visually drained.
        SMOOTH_STREAM_MAX_LAG_SECONDS / 3;
    const rate = Math.max(this.baseRate, backlog / lagSeconds);
    const budget = this.carry + (rate * deltaMs) / 1000;
    const advance = Math.min(backlog, Math.floor(budget));
    this.carry = budget - advance;

    if (advance > 0) {
      let index = this.displayedIndex;
      let advanced = 0;
      while (advanced < advance && index < this.targetText.length) {
        const codePoint = this.targetText.codePointAt(index);
        index += codePoint !== undefined && codePoint > 0xffff ? 2 : 1;
        advanced += 1;
      }
      this.displayedText += this.targetText.slice(this.displayedIndex, index);
      this.displayedIndex = index;
      this.displayedUnits += advanced;
    }

    const done = this.displayedUnits >= this.targetUnits;
    if (done && !this.isStreaming) {
      this.finishing = false;
      this.wasStreaming = false;
    }
    return {
      text: this.displayedText,
      isSmoothing: !done && (this.isStreaming || this.finishing),
      done,
    };
  }
}

/**
 * rAF owner around the pure clock. `stop()` is intentionally resumable so a
 * StrictMode effect cleanup cannot permanently disable a mounted component.
 */
export class SmoothStreamPlayer {
  private readonly clock: SmoothStreamClock;
  private readonly onState: (state: SmoothStreamState) => void;
  private readonly scheduler: SmoothStreamScheduler;
  private frameId: number | null = null;

  constructor(
    targetText: string,
    isStreaming: boolean,
    onState: (state: SmoothStreamState) => void,
    scheduler: SmoothStreamScheduler = browserScheduler,
    baseRate = SMOOTH_STREAM_BASE_RATE,
    maxBacklog = Number.POSITIVE_INFINITY,
  ) {
    this.onState = onState;
    this.scheduler = scheduler;
    this.clock = new SmoothStreamClock(
      targetText,
      isStreaming,
      baseRate,
      maxBacklog,
    );
  }

  get currentText(): string {
    return this.clock.currentText;
  }

  sync(targetText: string, isStreaming: boolean): SmoothStreamState {
    const state = this.clock.sync(targetText, isStreaming);
    this.onState(state);
    if (this.frameId === null && this.clock.needsFrame) {
      // Only a fresh burst resets the frame clock; regular rAF ticks must
      // measure the real elapsed frame time.
      this.clock.beginFrame();
    }
    this.ensureFrame();
    return state;
  }

  stop(): void {
    if (this.frameId === null) {
      return;
    }
    this.scheduler.cancel(this.frameId);
    this.frameId = null;
  }

  private ensureFrame(): void {
    if (this.frameId !== null || !this.clock.needsFrame) {
      return;
    }
    this.frameId = this.scheduler.request((timestamp) => {
      this.frameId = null;
      const state = this.clock.nextFrame(timestamp);
      this.onState({ text: state.text, isSmoothing: state.isSmoothing });
      if (!state.done) {
        this.ensureFrame();
      }
    });
  }
}

/**
 * Smooth a streaming text target at a stable reading rate while catching up
 * bursts within roughly 350ms. Historical/static text is returned immediately.
 */
export function useSmoothStreamText(
  targetText: string,
  isStreaming: boolean,
  maxBacklog = Number.POSITIVE_INFINITY,
): SmoothStreamState {
  const [state, setState] = useState<SmoothStreamState>(() => ({
    text: targetText,
    isSmoothing: false,
  }));
  const [player] = useState(
    () =>
      new SmoothStreamPlayer(
        targetText,
        isStreaming,
        setState,
        browserScheduler,
        SMOOTH_STREAM_BASE_RATE,
        maxBacklog,
      ),
  );

  useEffect(() => {
    player.sync(targetText, isStreaming);
  }, [isStreaming, player, targetText]);

  useEffect(
    () => () => {
      player.stop();
    },
    [player],
  );

  return state;
}
