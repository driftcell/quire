import type { SessionSummary } from "@/contracts/session-protocol";

export type SessionGroupKey =
  "pinned" | "today" | "yesterday" | "thisWeek" | "earlier";

export interface SessionGroup {
  key: SessionGroupKey;
  label: string;
  items: SessionSummary[];
}

const DATE_GROUP_ORDER: readonly Exclude<SessionGroupKey, "pinned">[] = [
  "today",
  "yesterday",
  "thisWeek",
  "earlier",
];

const GROUP_LABELS: Record<SessionGroupKey, string> = {
  pinned: "置顶",
  today: "今天",
  yesterday: "昨天",
  thisWeek: "本周",
  earlier: "更早",
};

const DAY_MS = 86_400_000;

const shanghaiDayFormatter = new Intl.DateTimeFormat("en-US", {
  timeZone: "Asia/Shanghai",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

function timestampMs(value: string | null | undefined): number {
  if (!value) return 0;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function shanghaiCalendarDay(value: Date): number | undefined {
  if (!Number.isFinite(value.getTime())) return undefined;
  const parts = shanghaiDayFormatter.formatToParts(value);
  const year = Number(parts.find((part) => part.type === "year")?.value);
  const month = Number(parts.find((part) => part.type === "month")?.value);
  const day = Number(parts.find((part) => part.type === "day")?.value);
  if (!year || !month || !day) return undefined;
  return Math.floor(Date.UTC(year, month - 1, day) / DAY_MS);
}

function startOfWeek(day: number): number {
  const weekday = new Date(day * DAY_MS).getUTCDay();
  return day - ((weekday + 6) % 7);
}

function compareRecency(left: SessionSummary, right: SessionSummary): number {
  const updatedDiff =
    timestampMs(right.updated_at || right.created_at) -
    timestampMs(left.updated_at || left.created_at);
  return updatedDiff || left.session_id.localeCompare(right.session_id);
}

function comparePinned(left: SessionSummary, right: SessionSummary): number {
  const pinnedDiff = timestampMs(right.pinned_at) - timestampMs(left.pinned_at);
  return pinnedDiff || compareRecency(left, right);
}

/**
 * Split sessions into the pinned group plus Shanghai-calendar date groups.
 * Pinned sessions are excluded from date groups; the pinned group is omitted
 * entirely when empty.
 */
export function groupSessionsForSidebar(
  sessions: SessionSummary[],
): SessionGroup[] {
  const pinned = sessions
    .filter((session) => Boolean(session.pinned_at))
    .sort(comparePinned);
  const unpinned = sessions
    .filter((session) => !session.pinned_at)
    .sort(compareRecency);

  const today = shanghaiCalendarDay(new Date());
  const weekStart = today === undefined ? undefined : startOfWeek(today);
  const groups: Record<Exclude<SessionGroupKey, "pinned">, SessionSummary[]> = {
    today: [],
    yesterday: [],
    thisWeek: [],
    earlier: [],
  };

  for (const session of unpinned) {
    const day = shanghaiCalendarDay(
      new Date(session.updated_at || session.created_at),
    );
    let key: Exclude<SessionGroupKey, "pinned"> = "earlier";
    if (day !== undefined && today !== undefined && weekStart !== undefined) {
      if (day >= today) {
        key = "today";
      } else if (day === today - 1) {
        key = "yesterday";
      } else if (day >= weekStart) {
        key = "thisWeek";
      }
    }
    groups[key].push(session);
  }

  const result: SessionGroup[] = [];
  if (pinned.length > 0) {
    result.push({
      key: "pinned",
      label: GROUP_LABELS.pinned,
      items: pinned,
    });
  }
  for (const key of DATE_GROUP_ORDER) {
    if (groups[key].length > 0) {
      result.push({ key, label: GROUP_LABELS[key], items: groups[key] });
    }
  }
  return result;
}
