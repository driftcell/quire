import type { ChartRequirements } from "@/contracts/report-protocol";

type BuiltinMapName = "china" | "world";

type MapModule = { default?: unknown };

const MAP_LOADERS: Record<BuiltinMapName, () => Promise<MapModule>> = {
  china: () => import("../assets/maps/china.json"),
  world: () => import("../assets/maps/world.json"),
};

/**
 * Minimal ECharts surface used by the lazy map registry. Keeping this
 * structural lets tests pass a tiny stub without importing the full library.
 */
export interface EchartsMapRegistry {
  getMap(mapName: string): unknown;
  registerMap(mapName: string, geoJson: unknown): void;
}

const loadCache = new WeakMap<
  EchartsMapRegistry,
  Map<BuiltinMapName, Promise<void>>
>();

function cacheFor(
  registry: EchartsMapRegistry,
): Map<BuiltinMapName, Promise<void>> {
  const existing = loadCache.get(registry);
  if (existing) {
    return existing;
  }
  const created = new Map<BuiltinMapName, Promise<void>>();
  loadCache.set(registry, created);
  return created;
}

function normalizeMapName(name: string): string {
  return name.trim().toLowerCase();
}

function builtinMapName(name: string): BuiltinMapName | null {
  const normalized = normalizeMapName(name);
  return normalized === "china" || normalized === "world" ? normalized : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function collectMapNames(value: unknown, names: string[]): void {
  if (Array.isArray(value)) {
    for (const entry of value) {
      collectMapNames(entry, names);
    }
    return;
  }
  if (!isRecord(value)) {
    return;
  }
  const map = value.map;
  if (typeof map === "string" && map.trim().length > 0) {
    names.push(map.trim());
  }
}

/**
 * Whether a map can be loaded from the bundled `china` / `world` GeoJSON.
 * Matching is case-insensitive and normalizes to the lowercase registered name.
 */
export function isSupportedBuiltinMap(name: string): boolean {
  return builtinMapName(name) !== null;
}

/**
 * Collect every map referenced by chart requirements, `geo.map` and
 * `series[*].map`. Supported names are normalized to `china` / `world`;
 * unknown names keep their original spelling so the UI can report them.
 */
export function extractRequiredMaps(
  option: Record<string, unknown>,
  requirements?: ChartRequirements,
): string[] {
  const names: string[] = [];
  for (const name of requirements?.required_maps ?? []) {
    if (typeof name === "string" && name.trim().length > 0) {
      names.push(name.trim());
    }
  }
  collectMapNames(option.geo, names);
  collectMapNames(option.series, names);

  const seen = new Set<string>();
  const result: string[] = [];
  for (const name of names) {
    const key = normalizeMapName(name);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    result.push(builtinMapName(key) ?? name);
  }
  return result;
}

async function loadBuiltinMap(
  registry: EchartsMapRegistry,
  name: BuiltinMapName,
): Promise<void> {
  if (registry.getMap(name)) {
    return;
  }
  const cache = cacheFor(registry);
  const pending = cache.get(name);
  if (pending) {
    return pending;
  }

  const promise = (async () => {
    const module = await MAP_LOADERS[name]();
    const geoJson = module.default ?? module;
    if (!isRecord(geoJson)) {
      throw new Error(`内置地图 \`${name}\` 数据格式无效`);
    }
    // Another caller may have registered the same map while this import was
    // in flight, so check once more before mutating ECharts' global registry.
    if (!registry.getMap(name)) {
      registry.registerMap(name, geoJson);
    }
  })().finally(() => {
    cache.delete(name);
  });

  cache.set(name, promise);
  return promise;
}

/**
 * Register every supported built-in map before a chart option is applied.
 * Concurrent callers share one import/registration promise per map. Unknown
 * names are returned unchanged and are not treated as an error.
 */
export async function ensureEchartsMaps(
  registry: EchartsMapRegistry,
  mapNames: string[],
): Promise<string[]> {
  const supported: BuiltinMapName[] = [];
  const unknown: string[] = [];
  const seen = new Set<string>();

  for (const rawName of mapNames) {
    const key = normalizeMapName(rawName);
    if (!key || seen.has(key)) {
      continue;
    }
    seen.add(key);
    const builtin = builtinMapName(key);
    if (builtin) {
      supported.push(builtin);
    } else {
      unknown.push(rawName.trim());
    }
  }

  await Promise.all(supported.map((name) => loadBuiltinMap(registry, name)));
  return unknown;
}
