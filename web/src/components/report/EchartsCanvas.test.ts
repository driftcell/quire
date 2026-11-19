import { describe, expect, test } from "bun:test";
import type { EchartsBlock } from "@/lib/document";
import { buildEffectiveOption } from "./EchartsCanvas";

function chart(option: Record<string, unknown>): EchartsBlock {
  return {
    title: "Revenue overview",
    height: 360,
    option,
    fallback_summary: "Revenue rose.",
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function nested(value: unknown, ...keys: string[]): unknown {
  let current = value;
  for (const key of keys) {
    if (!isRecord(current)) {
      return undefined;
    }
    current = current[key];
  }
  return current;
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

describe("buildEffectiveOption", () => {
  test("preserves a multi-grid layout and styles every axis entry", () => {
    const grid = [
      {
        left: "4%",
        right: "54%",
        top: 36,
        bottom: 24,
        containLabel: true,
      },
      {
        left: "54%",
        right: "4%",
        top: 36,
        bottom: 24,
        containLabel: true,
      },
    ];
    const effective = buildEffectiveOption(
      chart({
        grid,
        xAxis: [{ gridIndex: 0 }, { gridIndex: 1 }],
        yAxis: [{ gridIndex: 0 }, { gridIndex: 1 }],
        series: [
          { type: "bar", xAxisIndex: 0, yAxisIndex: 0, data: [1, 2] },
          { type: "line", xAxisIndex: 1, yAxisIndex: 1, data: [3, 4] },
        ],
      }),
    );

    expect(effective.grid).toEqual(grid);
    const xAxes = arrayValue(effective.xAxis);
    const yAxes = arrayValue(effective.yAxis);
    expect(xAxes).toHaveLength(2);
    expect(yAxes).toHaveLength(2);
    for (const axis of [...xAxes, ...yAxes]) {
      expect(nested(axis, "axisLine", "lineStyle", "color")).toBe("#CBD5E1");
      expect(nested(axis, "axisLabel", "color")).toBe("#475569");
      expect(nested(axis, "splitLine", "lineStyle", "color")).toBe("#E2E8F0");
      expect(nested(axis, "splitLine", "lineStyle", "type")).toBe("dashed");
    }
  });

  test("keeps multi-panel title arrays intact", () => {
    const title = [
      {
        text: "华东区",
        left: "4%",
        top: 4,
        textStyle: { fontSize: 12, color: "#334155" },
      },
      {
        text: "华南区",
        left: "54%",
        top: 4,
        textStyle: { fontSize: 12, color: "#334155" },
      },
    ];
    const effective = buildEffectiveOption(
      chart({ title, series: [{ type: "bar", data: [1, 2] }] }),
    );

    expect(effective.title).toEqual(title);
  });

  test("still injects a default grid for a single Cartesian chart", () => {
    const effective = buildEffectiveOption(
      chart({
        xAxis: { data: ["Q1", "Q2"] },
        yAxis: {},
        series: [{ type: "bar", data: [1, 2] }],
      }),
    );

    expect(effective.grid).toEqual({
      containLabel: true,
      left: 12,
      right: 16,
      top: 24,
      bottom: 16,
    });
  });
});
