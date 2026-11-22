import { describe, expect, test } from "bun:test";
import { LatestRevisionSync } from "@/lib/latestRevisionSync";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("LatestRevisionSync", () => {
  test("runs a single flight and one trailing latest-revision sync", async () => {
    let applied = 1;
    const calls: number[] = [];
    const gates: Array<ReturnType<typeof deferred<number>>> = [];
    const sync = new LatestRevisionSync(
      () => applied,
      (from) => {
        calls.push(from);
        const gate = deferred<number>();
        gates.push(gate);
        return gate.promise.then((next) => {
          applied = next;
          return next;
        });
      },
    );

    const first = sync.request(2);
    const joined = sync.request(5);
    expect(joined).toBe(first);
    expect(calls).toEqual([1]);

    gates[0]?.resolve(2);
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toEqual([1, 2]);

    gates[1]?.resolve(5);
    await Promise.all([first, joined]);
    expect(applied).toBe(5);
    expect(calls).toEqual([1, 2]);

    await sync.request(5);
    expect(calls).toEqual([1, 2]);
  });

  test("force runs a same-revision recovery without requiring progress", async () => {
    let calls = 0;
    const sync = new LatestRevisionSync(
      () => 7,
      async (from) => {
        calls += 1;
        return from;
      },
    );
    await sync.request(7, true);
    expect(calls).toBe(1);
  });

  test("rejects a no-progress sync and permits a later retry", async () => {
    let applied = 3;
    let attempts = 0;
    const sync = new LatestRevisionSync(
      () => applied,
      async (from) => {
        attempts += 1;
        if (attempts === 1) return from;
        applied = 4;
        return applied;
      },
    );

    let failure: unknown;
    try {
      await sync.request(4);
    } catch (error) {
      failure = error;
    }
    expect(failure).toBeInstanceOf(Error);
    if (failure instanceof Error) {
      expect(failure.message).toContain("no revision progress");
    }
    await sync.request(4);
    expect(attempts).toBe(2);
    expect(applied).toBe(4);
  });
});
