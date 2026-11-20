import { describe, expect, test } from "bun:test";
import * as echarts from "echarts";
import chinaMap from "@/assets/maps/china.json";
import worldMap from "@/assets/maps/world.json";
import type { ChartRequirements } from "@/contracts/report-protocol";
import {
  ensureEchartsMaps,
  extractRequiredMaps,
  isSupportedBuiltinMap,
  type EchartsMapRegistry,
} from "./echartsMaps";

type Position = [number, number];

interface MapFeature {
  properties?: {
    name?: string;
    fullname?: string;
  };
  geometry?: {
    type?: string;
    coordinates?: unknown;
  };
}

interface MapCollection {
  features: MapFeature[];
}

function isMapCollection(value: unknown): value is MapCollection {
  return (
    typeof value === "object" &&
    value !== null &&
    "features" in value &&
    Array.isArray(value.features)
  );
}

function asMapCollection(value: unknown, label: string): MapCollection {
  if (!isMapCollection(value)) {
    throw new Error(`${label} is not a GeoJSON FeatureCollection`);
  }
  return value;
}

const china = asMapCollection(chinaMap, "china.json");
const world = asMapCollection(worldMap, "world.json");

function positions(value: unknown): Position[] {
  if (!Array.isArray(value)) {
    return [];
  }
  if (typeof value[0] === "number" && typeof value[1] === "number") {
    return [[value[0], value[1]]];
  }
  return value.flatMap(positions);
}

function featurePositions(feature: MapFeature | undefined): Position[] {
  return positions(feature?.geometry?.coordinates);
}

describe("built-in ECharts maps", () => {
  test("recognizes supported map names case-insensitively", () => {
    expect(isSupportedBuiltinMap("china")).toBe(true);
    expect(isSupportedBuiltinMap("China")).toBe(true);
    expect(isSupportedBuiltinMap("WORLD")).toBe(true);
    expect(isSupportedBuiltinMap("world")).toBe(true);
    expect(isSupportedBuiltinMap("mars")).toBe(false);
    expect(isSupportedBuiltinMap(" guangdong ")).toBe(false);
  });

  test("extracts maps from requirements, geo and series", () => {
    const option: Record<string, unknown> = {
      geo: { map: "world" },
      series: [
        { type: "map", map: "china" },
        { type: "bar", data: [1] },
      ],
    };

    expect(extractRequiredMaps(option)).toEqual(["world", "china"]);

    const requirements: ChartRequirements = {
      required_maps: ["China", "mars"],
    };
    expect(extractRequiredMaps(option, requirements)).toEqual([
      "china",
      "mars",
      "world",
    ]);
  });

  test("deduplicates case variants and ignores empty map names", () => {
    expect(
      extractRequiredMaps({
        series: [{ map: "China" }, { map: "china" }, { map: " " }],
      }),
    ).toEqual(["china"]);
  });

  test("loads and registers a supported map once", async () => {
    const registered: string[] = [];
    const registry: EchartsMapRegistry = {
      getMap: () => undefined,
      registerMap: (name) => {
        registered.push(name);
      },
    };

    const unresolved = await ensureEchartsMaps(registry, [
      "China",
      "guangdong",
      "mars",
      "china",
    ]);

    expect(unresolved).toEqual(["guangdong", "mars"]);
    expect(registered).toEqual(["china"]);
  });

  test("shares one registration across concurrent callers", async () => {
    let registrations = 0;
    const registry: EchartsMapRegistry = {
      getMap: () => undefined,
      registerMap: () => {
        registrations += 1;
      },
    };

    await Promise.all([
      ensureEchartsMaps(registry, ["china"]),
      ensureEchartsMaps(registry, ["China"]),
    ]);

    expect(registrations).toBe(1);
  });

  test("registers supported maps with the real ECharts registry", async () => {
    await ensureEchartsMaps(echarts, ["China"]);
    expect(echarts.getMap("china")).toBeDefined();

    await ensureEchartsMaps(echarts, ["china"]);
    expect(echarts.getMap("china")).toBeDefined();

    await ensureEchartsMaps(echarts, ["World"]);
    expect(echarts.getMap("world")).toBeDefined();
  });

  test("renders a map series after lazy registration", async () => {
    await ensureEchartsMaps(echarts, ["china"]);
    const chart = echarts.init(null, null, {
      renderer: "svg",
      ssr: true,
      width: 400,
      height: 300,
    });
    try {
      expect(() =>
        chart.setOption({
          series: [
            {
              type: "map",
              map: "china",
              data: [{ name: "广东", value: 1 }],
            },
          ],
        }),
      ).not.toThrow();
    } finally {
      chart.dispose();
    }
  });

  test("china map keeps all provinces plus the South China Sea line", () => {
    const features = china.features ?? [];
    const names = features
      .map((feature) => feature.properties?.name)
      .filter((name): name is string => Boolean(name));
    const provinceNames = names.filter((name) => name !== "十段线");

    expect(provinceNames).toHaveLength(34);
    for (const name of [
      "北京",
      "广东",
      "内蒙古",
      "新疆",
      "台湾",
      "香港",
      "澳门",
    ]) {
      expect(provinceNames).toContain(name);
    }

    const nineDashLine = features.find((feature) => {
      const name = feature.properties?.name ?? "";
      return (
        name.includes("九段线") ||
        name.includes("十段线") ||
        name.includes("南海")
      );
    });
    expect(nineDashLine?.properties?.name).toBe("十段线");
    expect(nineDashLine?.properties?.fullname).toContain("南海");

    const hainan = features.find(
      (feature) => feature.properties?.name === "海南",
    );
    const hainanPositions = featurePositions(hainan);
    expect(hainanPositions.length).toBeGreaterThan(0);
    expect(Math.min(...hainanPositions.map(([, lat]) => lat))).toBeLessThan(17);
  });

  test("world map exposes standard English country names", () => {
    const names = (world.features ?? []).map(
      (feature) => feature.properties?.name,
    );
    expect(names).toContain("China");
    expect(names).toContain("United States");
    expect(names).toContain("United Kingdom");
  });
});
