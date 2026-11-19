import type { ECharts } from "echarts";

interface GridMargins {
  left: number;
  right: number;
}

interface ContentRect {
  x: number;
  width: number;
}

interface BoundsRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface GridRect {
  x: number;
  width: number;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

/**
 * Safely call an optional private ECharts/ZRender method without asserting the
 * whole object to a narrow private interface.
 */
function callMethod(target: unknown, key: string, ...args: unknown[]): unknown {
  if (!isRecord(target)) {
    return undefined;
  }
  const method = target[key];
  if (typeof method !== "function") {
    return undefined;
  }
  try {
    return Reflect.apply(method, target, args);
  } catch {
    return undefined;
  }
}

/**
 * Accept both ECharts' normalized one-element arrays and a plain object for
 * single-component options such as `grid`, `xAxis` and `yAxis`.
 */
function singleComponent(value: unknown): Record<string, unknown> | undefined {
  if (isRecord(value)) {
    return value;
  }
  if (Array.isArray(value) && value.length === 1) {
    const [first] = value;
    return isRecord(first) ? first : undefined;
  }
  return undefined;
}

/**
 * Pure margin calculation. Returns `undefined` when the content is already
 * centered or when no valid adjustment can be made.
 */
export function centeredGridMargins(
  margins: GridMargins,
  content: ContentRect,
  chartWidth: number,
): GridMargins | undefined {
  const { left, right } = margins;
  if (
    !isFiniteNumber(chartWidth) ||
    chartWidth <= 0 ||
    !isFiniteNumber(left) ||
    !isFiniteNumber(right) ||
    left < 0 ||
    right < 0 ||
    !isFiniteNumber(content.x) ||
    !isFiniteNumber(content.width) ||
    content.width <= 0
  ) {
    return undefined;
  }

  const offset = content.x + content.width / 2 - chartWidth / 2;
  if (Math.abs(offset) < 1) {
    return undefined;
  }

  const adjustment = Math.max(-right, Math.min(left, offset));
  if (Math.abs(adjustment) < 0.01) {
    return undefined;
  }

  const nextLeft = Math.round((left - adjustment) * 100) / 100;
  const nextRight = Math.round((right + adjustment) * 100) / 100;
  if (Math.abs(nextLeft - left) < 0.01 && Math.abs(nextRight - right) < 0.01) {
    return undefined;
  }
  return { left: nextLeft, right: nextRight };
}

function readGridRect(chart: ECharts): GridRect | undefined {
  const model = callMethod(chart, "getModel");
  const component = callMethod(model, "getComponent", "grid");
  const coordinateSystem = isRecord(component)
    ? component.coordinateSystem
    : undefined;
  const rect = callMethod(coordinateSystem, "getRect");
  if (!isRecord(rect)) {
    return undefined;
  }
  const { x, width } = rect;
  if (isFiniteNumber(x) && isFiniteNumber(width) && width > 0) {
    return { x, width };
  }
  return undefined;
}

function toBoundsRect(value: unknown): BoundsRect | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const { x, y, width, height } = value;
  if (
    !isFiniteNumber(x) ||
    !isFiniteNumber(y) ||
    !isFiniteNumber(width) ||
    !isFiniteNumber(height) ||
    width < 0 ||
    height < 0
  ) {
    return undefined;
  }
  return { x, y, width, height };
}

function transformedBounds(
  rect: BoundsRect,
  transform: number[] | undefined,
): BoundsRect {
  if (!Array.isArray(transform) || transform.length !== 6) {
    return rect;
  }
  const [a = 1, b = 0, c = 0, d = 1, e = 0, f = 0] = transform;
  const points = [
    { x: a * rect.x + c * rect.y + e, y: b * rect.x + d * rect.y + f },
    {
      x: a * (rect.x + rect.width) + c * rect.y + e,
      y: b * (rect.x + rect.width) + d * rect.y + f,
    },
    {
      x: a * rect.x + c * (rect.y + rect.height) + e,
      y: b * rect.x + d * (rect.y + rect.height) + f,
    },
    {
      x: a * (rect.x + rect.width) + c * (rect.y + rect.height) + e,
      y: b * (rect.x + rect.width) + d * (rect.y + rect.height) + f,
    },
  ];
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (const point of points) {
    minX = Math.min(minX, point.x);
    minY = Math.min(minY, point.y);
    maxX = Math.max(maxX, point.x);
    maxY = Math.max(maxY, point.y);
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

function measureContent(
  chart: ECharts,
  chartWidth: number,
  chartHeight: number,
): ContentRect | undefined {
  const zr = callMethod(chart, "getZr");
  const storage = isRecord(zr) ? zr.storage : undefined;
  const elements = callMethod(storage, "getDisplayList", true);
  if (!Array.isArray(elements)) {
    return undefined;
  }

  let minX = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  for (const element of elements) {
    const style = isRecord(element) ? element.style : undefined;
    if (isRecord(style) && style.opacity === 0) {
      continue;
    }
    const rect = toBoundsRect(callMethod(element, "getBoundingRect"));
    if (!rect || rect.width <= 0) {
      continue;
    }
    const elementType = isRecord(element) ? element.type : undefined;
    if (
      elementType === "rect" &&
      rect.x <= 0 &&
      rect.y <= 0 &&
      rect.x + rect.width >= chartWidth &&
      rect.y + rect.height >= chartHeight
    ) {
      continue;
    }
    const transformValue = callMethod(element, "getComputedTransform");
    const transform = Array.isArray(transformValue)
      ? transformValue.filter(
          (value): value is number => typeof value === "number",
        )
      : undefined;
    const bounds = transformedBounds(rect, transform);
    if (!isFiniteNumber(bounds.x) || !isFiniteNumber(bounds.width)) {
      continue;
    }
    minX = Math.min(minX, bounds.x);
    maxX = Math.max(maxX, bounds.x + bounds.width);
  }

  if (!Number.isFinite(minX) || !Number.isFinite(maxX) || maxX <= minX) {
    return undefined;
  }
  return { x: minX, width: maxX - minX };
}

/**
 * Horizontally center the rendered contents of a standard single-grid
 * Cartesian chart without changing the plot width.
 *
 * The first pass may switch `containLabel: true` to explicit pixel margins.
 * That is required because a small initial `grid.left` (for example 12px) is
 * not the real left margin once long Y-axis labels are included.
 */
export function centerReportChartGrid(chart: ECharts): boolean {
  try {
    const option = callMethod(chart, "getOption");
    if (!isRecord(option)) {
      return false;
    }

    const grid = singleComponent(option.grid);
    const xAxis = singleComponent(option.xAxis);
    const yAxis = singleComponent(option.yAxis);
    if (!grid || !xAxis || !yAxis || grid.width !== undefined) {
      return false;
    }

    const chartWidth = chart.getWidth();
    const chartHeight = chart.getHeight();
    if (
      !isFiniteNumber(chartWidth) ||
      chartWidth <= 0 ||
      !isFiniteNumber(chartHeight) ||
      chartHeight <= 0
    ) {
      return false;
    }

    const wasContainLabel = grid.containLabel === true;
    let left: number;
    let right: number;
    if (wasContainLabel) {
      const rect = readGridRect(chart);
      if (!rect) {
        return false;
      }
      const actualLeft = rect.x;
      const actualRight = chartWidth - (rect.x + rect.width);
      if (
        !isFiniteNumber(actualLeft) ||
        !isFiniteNumber(actualRight) ||
        actualLeft < 0 ||
        actualRight < 0
      ) {
        return false;
      }
      left = actualLeft;
      right = actualRight;
    } else {
      if (!isFiniteNumber(grid.left) || !isFiniteNumber(grid.right)) {
        return false;
      }
      left = grid.left;
      right = grid.right;
    }

    let adjusted = false;
    for (let pass = 0; pass < 2; pass += 1) {
      const content = measureContent(chart, chartWidth, chartHeight);
      if (!content) {
        break;
      }
      const margins = centeredGridMargins({ left, right }, content, chartWidth);
      if (!margins) {
        break;
      }
      chart.setOption({
        grid: {
          left: margins.left,
          right: margins.right,
          ...(wasContainLabel ? { containLabel: false } : {}),
        },
      });
      left = margins.left;
      right = margins.right;
      adjusted = true;
    }
    return adjusted;
  } catch {
    // The visual centering helper must never break chart rendering.
    return false;
  }
}
