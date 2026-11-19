import { describe, expect, test } from "bun:test";
import type { ECharts } from "echarts";
import {
  centerReportChartGrid,
  centeredGridMargins,
} from "./echartsGridCenter";

interface GridState {
  left: number | string;
  right: number | string;
  containLabel?: boolean;
}

function isGridState(value: unknown): value is GridState {
  return (
    typeof value === "object" &&
    value !== null &&
    "left" in value &&
    "right" in value
  );
}

function mockChart(options: {
  grid: unknown;
  xAxis?: unknown;
  yAxis?: unknown;
  chartWidth?: number;
  chartHeight?: number;
  getZr?: () => unknown;
  getModel?: () => unknown;
  onSetOption?: (option: { grid?: GridState }) => void;
}) {
  const calls: { grid?: GridState }[] = [];
  let grid = options.grid;
  const chart = {
    getOption: () => ({
      grid,
      xAxis: options.xAxis ?? [{ type: "category" }],
      yAxis: options.yAxis ?? [{ type: "value" }],
    }),
    getWidth: () => options.chartWidth ?? 628,
    getHeight: () => options.chartHeight ?? 360,
    getModel:
      options.getModel ??
      (() => ({
        getComponent: () => ({
          coordinateSystem: {
            getRect: () => ({ x: 120, width: 492 }),
          },
        }),
      })),
    getZr: options.getZr ?? (() => ({ storage: { getDisplayList: () => [] } })),
    setOption: (option: { grid?: GridState }) => {
      calls.push(option);
      if (!option.grid) {
        return;
      }
      const current = Array.isArray(grid) ? grid[0] : grid;
      if (!isGridState(current)) {
        grid = option.grid;
        return;
      }
      grid = Array.isArray(grid)
        ? [{ ...current, ...option.grid }]
        : { ...current, ...option.grid };
      options.onSetOption?.(option);
    },
  };
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- test double mirrors the ECharts surface used by the helper
  return { chart: chart as unknown as ECharts, calls };
}

describe("centeredGridMargins", () => {
  test("moves a right-heavy plot back to the visual center", () => {
    expect(
      centeredGridMargins(
        { left: 170, right: 80 },
        { x: 170, width: 378 },
        628,
      ),
    ).toEqual({ left: 125, right: 125 });
  });

  test("returns undefined when the content is already centered", () => {
    expect(
      centeredGridMargins({ left: 62, right: 62 }, { x: 62, width: 504 }, 628),
    ).toBeUndefined();
  });

  test("clamps the adjustment so both margins stay non-negative", () => {
    expect(
      centeredGridMargins({ left: 20, right: 30 }, { x: 200, width: 200 }, 400),
    ).toEqual({ left: 0, right: 50 });
  });
});

describe("centerReportChartGrid", () => {
  test("uses the real containLabel plot rect and preserves plot width", () => {
    const plotWidth = 492;
    let actualLeft = 120;
    const calls: { grid?: GridState }[] = [];
    const { chart } = mockChart({
      grid: [{ containLabel: true, left: 12, right: 16 }],
      getModel: () => ({
        getComponent: () => ({
          coordinateSystem: {
            getRect: () => ({ x: actualLeft, width: plotWidth }),
          },
        }),
      }),
      getZr: () => ({
        storage: {
          getDisplayList: () => [
            {
              type: "rect",
              getBoundingRect: () => ({
                x: actualLeft,
                y: 20,
                width: plotWidth,
                height: 200,
              }),
            },
          ],
        },
      }),
      onSetOption: (option) => {
        calls.push(option);
        if (option.grid && typeof option.grid.left === "number") {
          actualLeft = option.grid.left;
        }
      },
    });

    expect(centerReportChartGrid(chart)).toBe(true);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.grid).toMatchObject({
      left: 68,
      right: 68,
      containLabel: false,
    });
    expect(actualLeft).toBe(68);
    expect(628 - 68 - 68).toBe(plotWidth);
  });

  test("skips charts with multiple grids", () => {
    const { chart, calls } = mockChart({
      grid: [{ containLabel: true }, { containLabel: true }],
    });
    expect(centerReportChartGrid(chart)).toBe(false);
    expect(calls).toHaveLength(0);
  });

  test("skips pie charts without a single Cartesian axis pair", () => {
    const { chart, calls } = mockChart({
      grid: { left: 20, right: 20 },
      xAxis: undefined,
      yAxis: undefined,
    });
    expect(centerReportChartGrid(chart)).toBe(false);
    expect(calls).toHaveLength(0);
  });

  test("skips percentage margins when containLabel is not true", () => {
    const { chart, calls } = mockChart({
      grid: { left: "10%", right: "10%" },
    });
    expect(centerReportChartGrid(chart)).toBe(false);
    expect(calls).toHaveLength(0);
  });

  test("fails silently when private ZRender APIs throw", () => {
    const { chart, calls } = mockChart({
      grid: { left: 20, right: 20 },
      getZr: () => {
        throw new Error("zrender unavailable");
      },
    });
    expect(() => centerReportChartGrid(chart)).not.toThrow();
    expect(centerReportChartGrid(chart)).toBe(false);
    expect(calls).toHaveLength(0);
  });

  test("fails silently when the grid coordinate system is unavailable", () => {
    const { chart, calls } = mockChart({
      grid: [{ containLabel: true, left: 12, right: 16 }],
      getModel: () => ({
        getComponent: () => ({
          coordinateSystem: {},
        }),
      }),
    });
    expect(centerReportChartGrid(chart)).toBe(false);
    expect(calls).toHaveLength(0);
  });
});
