import { memo } from "react";
import { BookOpenIcon } from "lucide-react";
import { cn } from "cn";

export interface AgentIdentity {
  actor: string;
  label: string;
  role: string;
  monogram: string;
  bgClass: string;
  textClass: string;
  ringClass: string;
  dotClass: string;
  iconKind: "main" | "research" | "analyst" | "writer" | "review" | "generic";
}

interface AvatarTheme {
  bg: string;
  face: string;
  accent: string;
  ink: string;
  bgClass: string;
  textClass: string;
  ringClass: string;
  dotClass: string;
}

/**
 * 12 curated Vercel / Linear / Boring-Avatars Beam harmonic palettes.
 * Each agent name hashes deterministically to a unique color triad and
 * geometric character expression that stays 100% consistent across renders.
 */
const MAIN_AVATAR_THEME: AvatarTheme = {
  bg: "#18181b",
  face: "#3f3f46",
  accent: "#38bdf8",
  ink: "#fafafa",
  bgClass: "bg-zinc-900",
  textClass: "text-zinc-50",
  ringClass: "ring-border/70",
  dotClass: "bg-zinc-900",
};

const AVATAR_THEMES: AvatarTheme[] = [
  {
    bg: "#18181b",
    face: "#3f3f46",
    accent: "#38bdf8",
    ink: "#fafafa",
    bgClass: "bg-zinc-900",
    textClass: "text-zinc-50",
    ringClass: "ring-border/70",
    dotClass: "bg-zinc-900",
  },
  {
    bg: "#e0f2fe",
    face: "#0284c7",
    accent: "#38bdf8",
    ink: "#ffffff",
    bgClass: "bg-sky-500/12",
    textClass: "text-sky-700",
    ringClass: "ring-sky-500/25",
    dotClass: "bg-sky-500",
  },
  {
    bg: "#d1fae5",
    face: "#059669",
    accent: "#34d399",
    ink: "#ffffff",
    bgClass: "bg-emerald-500/12",
    textClass: "text-emerald-700",
    ringClass: "ring-emerald-500/25",
    dotClass: "bg-emerald-500",
  },
  {
    bg: "#fef3c7",
    face: "#d97706",
    accent: "#fbbf24",
    ink: "#ffffff",
    bgClass: "bg-amber-500/12",
    textClass: "text-amber-700",
    ringClass: "ring-amber-500/25",
    dotClass: "bg-amber-500",
  },
  {
    bg: "#ede9fe",
    face: "#7c3aed",
    accent: "#a78bfa",
    ink: "#ffffff",
    bgClass: "bg-violet-500/12",
    textClass: "text-violet-700",
    ringClass: "ring-violet-500/25",
    dotClass: "bg-violet-500",
  },
  {
    bg: "#ffe4e6",
    face: "#e11d48",
    accent: "#fb7185",
    ink: "#ffffff",
    bgClass: "bg-rose-500/12",
    textClass: "text-rose-700",
    ringClass: "ring-rose-500/25",
    dotClass: "bg-rose-500",
  },
  {
    bg: "#ccfbf1",
    face: "#0d9488",
    accent: "#2dd4bf",
    ink: "#ffffff",
    bgClass: "bg-teal-500/12",
    textClass: "text-teal-700",
    ringClass: "ring-teal-500/25",
    dotClass: "bg-teal-500",
  },
  {
    bg: "#e0e7ff",
    face: "#4f46e5",
    accent: "#818cf8",
    ink: "#ffffff",
    bgClass: "bg-indigo-500/12",
    textClass: "text-indigo-700",
    ringClass: "ring-indigo-500/25",
    dotClass: "bg-indigo-500",
  },
  {
    bg: "#fce7f3",
    face: "#db2777",
    accent: "#f472b6",
    ink: "#ffffff",
    bgClass: "bg-pink-500/12",
    textClass: "text-pink-700",
    ringClass: "ring-pink-500/25",
    dotClass: "bg-pink-500",
  },
  {
    bg: "#ffedd5",
    face: "#ea580c",
    accent: "#fb923c",
    ink: "#ffffff",
    bgClass: "bg-orange-500/12",
    textClass: "text-orange-700",
    ringClass: "ring-orange-500/25",
    dotClass: "bg-orange-500",
  },
  {
    bg: "#ecfccb",
    face: "#65a30d",
    accent: "#a3e635",
    ink: "#ffffff",
    bgClass: "bg-lime-500/12",
    textClass: "text-lime-700",
    ringClass: "ring-lime-500/25",
    dotClass: "bg-lime-500",
  },
  {
    bg: "#cffafe",
    face: "#0891b2",
    accent: "#22d3ee",
    ink: "#ffffff",
    bgClass: "bg-cyan-500/12",
    textClass: "text-cyan-700",
    ringClass: "ring-cyan-500/25",
    dotClass: "bg-cyan-500",
  },
];

function fnv1aHash(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

export function resolveAgentIdentity(rawActor: string): AgentIdentity {
  const actor = rawActor.trim() || "main";
  const lower = actor.toLowerCase();

  if (lower === "main" || lower === "orchestrator" || lower === "主笔") {
    const mainTheme = MAIN_AVATAR_THEME;
    return {
      actor,
      label: "主编",
      role: "总控编排与统筹",
      monogram: "Q",
      bgClass: mainTheme.bgClass,
      textClass: mainTheme.textClass,
      ringClass: mainTheme.ringClass,
      dotClass: mainTheme.dotClass,
      iconKind: "main",
    };
  }

  const hash = fnv1aHash(lower);
  const theme =
    AVATAR_THEMES[1 + (hash % (AVATAR_THEMES.length - 1))] ?? MAIN_AVATAR_THEME;

  if (
    lower.includes("research") ||
    lower.includes("search") ||
    lower.includes("调研") ||
    lower.includes("检索")
  ) {
    return {
      actor,
      label: actor,
      role: "资料检索与核验",
      monogram: "研",
      bgClass: theme.bgClass,
      textClass: theme.textClass,
      ringClass: theme.ringClass,
      dotClass: theme.dotClass,
      iconKind: "research",
    };
  }

  if (
    lower.includes("analyst") ||
    lower.includes("data") ||
    lower.includes("chart") ||
    lower.includes("分析") ||
    lower.includes("图表")
  ) {
    return {
      actor,
      label: actor,
      role: "数据分析与可视化",
      monogram: "析",
      bgClass: theme.bgClass,
      textClass: theme.textClass,
      ringClass: theme.ringClass,
      dotClass: theme.dotClass,
      iconKind: "analyst",
    };
  }

  if (
    lower.includes("writer") ||
    lower.includes("author") ||
    lower.includes("draft") ||
    lower.includes("撰稿") ||
    lower.includes("撰写")
  ) {
    return {
      actor,
      label: actor,
      role: "章节撰写",
      monogram: "撰",
      bgClass: theme.bgClass,
      textClass: theme.textClass,
      ringClass: theme.ringClass,
      dotClass: theme.dotClass,
      iconKind: "writer",
    };
  }

  if (
    lower.includes("review") ||
    lower.includes("qa") ||
    lower.includes("audit") ||
    lower.includes("审校") ||
    lower.includes("审核")
  ) {
    return {
      actor,
      label: actor,
      role: "质量审校",
      monogram: "审",
      bgClass: theme.bgClass,
      textClass: theme.textClass,
      ringClass: theme.ringClass,
      dotClass: theme.dotClass,
      iconKind: "review",
    };
  }

  const firstChar = Array.from(actor)[0]?.toUpperCase() || "A";
  return {
    actor,
    label: actor,
    role: "协作子代理",
    monogram: firstChar,
    bgClass: theme.bgClass,
    textClass: theme.textClass,
    ringClass: theme.ringClass,
    dotClass: theme.dotClass,
    iconKind: "generic",
  };
}

/**
 * Deterministic Boring-Avatars Beam style character SVG generator.
 * Produces a unique, expressive, friendly geometric face for each agent
 * identifier without any random runtime drift.
 */
function DeterministicBeamSvg({ actor }: { actor: string }) {
  const lower = (actor.trim() || "main").toLowerCase();
  const isMain =
    lower === "main" || lower === "orchestrator" || lower === "主笔";
  const hash = fnv1aHash(lower);
  const theme = isMain
    ? MAIN_AVATAR_THEME
    : (AVATAR_THEMES[1 + (hash % (AVATAR_THEMES.length - 1))] ??
      MAIN_AVATAR_THEME);

  const tx = ((hash & 0x07) - 3) * 1.1;
  const ty = (((hash >> 3) & 0x07) - 3) * 1.1;
  const rotate = (((hash >> 6) & 0x0f) - 7) * 1.5;
  const eyeSpread = 3.6 + ((hash >> 10) & 0x03) * 0.55;
  const eyeY = 14.5 + ((hash >> 12) & 0x03) * 0.5;
  const mouthSmile = (hash >> 14) & 0x01;
  const orbX = 6 + ((hash >> 15) & 0x0f);
  const orbY = 6 + ((hash >> 19) & 0x0f);

  return (
    <svg
      viewBox="0 0 36 36"
      fill="none"
      role="img"
      aria-hidden="true"
      className="size-full overflow-hidden rounded-full"
    >
      <rect width="36" height="36" rx="18" fill={theme.bg} />
      <circle cx={orbX} cy={orbY} r="10" fill={theme.accent} opacity="0.32" />
      <g
        transform={`translate(${tx.toFixed(1)} ${ty.toFixed(1)}) rotate(${rotate.toFixed(1)} 18 18)`}
      >
        <rect
          x="5"
          y="7"
          width="26"
          height="26"
          rx="11"
          fill={theme.face}
        />
        <circle cx={18 - eyeSpread} cy={eyeY} r="1.65" fill={theme.ink} />
        <circle cx={18 + eyeSpread} cy={eyeY} r="1.65" fill={theme.ink} />
        {mouthSmile ? (
          <path
            d={`M 15 ${eyeY + 5} Q 18 ${eyeY + 8} 21 ${eyeY + 5}`}
            stroke={theme.ink}
            strokeWidth="1.65"
            strokeLinecap="round"
            fill="none"
          />
        ) : (
          <rect
            x="15.5"
            y={eyeY + 4.5}
            width="5"
            height="1.6"
            rx="0.8"
            fill={theme.ink}
          />
        )}
      </g>
    </svg>
  );
}

export const AgentAvatar = memo(function AgentAvatar({
  actor,
  size = "sm",
  className,
}: {
  actor: string;
  size?: "xs" | "sm" | "md";
  /** Kept optional for call-site compatibility; no floating indicator dot is rendered. */
  active?: boolean;
  className?: string;
}) {
  const identity = resolveAgentIdentity(actor);
  const boxSize =
    size === "xs"
      ? "size-5 text-[10px]"
      : size === "md"
        ? "size-7 text-xs"
        : "size-6 text-[11px]";

  return (
    <span
      data-slot="agent-avatar"
      data-actor={identity.actor}
      title={`${identity.label} · ${identity.role}`}
      className={cn(
        "relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full ring-1 ring-border/70 select-none",
        boxSize,
        className,
      )}
    >
      <DeterministicBeamSvg actor={identity.actor} />
    </span>
  );
});

export { BookOpenIcon };
