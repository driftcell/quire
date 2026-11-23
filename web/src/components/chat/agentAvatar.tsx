import { memo } from "react";
import {
  BarChart3Icon,
  BookOpenIcon,
  CheckCheckIcon,
  FilePenLineIcon,
  SearchIcon,
  SparklesIcon,
  UserIcon,
} from "lucide-react";
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

interface PaletteEntry {
  bgClass: string;
  textClass: string;
  ringClass: string;
  dotClass: string;
}

const SUBAGENT_PALETTE: PaletteEntry[] = [
  {
    bgClass: "bg-sky-500/12",
    textClass: "text-sky-700",
    ringClass: "ring-sky-500/25",
    dotClass: "bg-sky-500",
  },
  {
    bgClass: "bg-emerald-500/12",
    textClass: "text-emerald-700",
    ringClass: "ring-emerald-500/25",
    dotClass: "bg-emerald-500",
  },
  {
    bgClass: "bg-amber-500/12",
    textClass: "text-amber-700",
    ringClass: "ring-amber-500/25",
    dotClass: "bg-amber-500",
  },
  {
    bgClass: "bg-violet-500/12",
    textClass: "text-violet-700",
    ringClass: "ring-violet-500/25",
    dotClass: "bg-violet-500",
  },
  {
    bgClass: "bg-rose-500/12",
    textClass: "text-rose-700",
    ringClass: "ring-rose-500/25",
    dotClass: "bg-rose-500",
  },
  {
    bgClass: "bg-teal-500/12",
    textClass: "text-teal-700",
    ringClass: "ring-teal-500/25",
    dotClass: "bg-teal-500",
  },
];

function hashActor(actor: string): number {
  let hash = 0;
  for (let i = 0; i < actor.length; i += 1) {
    hash = (hash * 31 + actor.charCodeAt(i)) >>> 0;
  }
  return hash;
}

export function resolveAgentIdentity(rawActor: string): AgentIdentity {
  const actor = rawActor.trim() || "main";
  const lower = actor.toLowerCase();

  if (lower === "main" || lower === "orchestrator" || lower === "主笔") {
    return {
      actor,
      label: "主笔 Agent",
      role: "总控编排与统筹",
      monogram: "Q",
      bgClass: "bg-slate-900",
      textClass: "text-white",
      ringClass: "ring-slate-900/20",
      dotClass: "bg-slate-900",
      iconKind: "main",
    };
  }

  const palette: PaletteEntry = SUBAGENT_PALETTE[
    hashActor(lower) % SUBAGENT_PALETTE.length
  ] ?? {
    bgClass: "bg-sky-500/12",
    textClass: "text-sky-700",
    ringClass: "ring-sky-500/25",
    dotClass: "bg-sky-500",
  };

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
      ...palette,
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
      ...palette,
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
      ...palette,
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
      ...palette,
      iconKind: "review",
    };
  }

  const firstChar = Array.from(actor)[0]?.toUpperCase() || "A";
  return {
    actor,
    label: actor,
    role: "协作子代理",
    monogram: firstChar,
    ...palette,
    iconKind: "generic",
  };
}

function renderRoleIcon(
  iconKind: AgentIdentity["iconKind"],
  iconClass: string,
  monogram: string,
) {
  switch (iconKind) {
    case "main":
      return <SparklesIcon className={iconClass} aria-hidden="true" />;
    case "research":
      return <SearchIcon className={iconClass} aria-hidden="true" />;
    case "analyst":
      return <BarChart3Icon className={iconClass} aria-hidden="true" />;
    case "writer":
      return <FilePenLineIcon className={iconClass} aria-hidden="true" />;
    case "review":
      return <CheckCheckIcon className={iconClass} aria-hidden="true" />;
    default:
      return monogram ? (
        <span className="leading-none font-semibold">{monogram}</span>
      ) : (
        <UserIcon className={iconClass} aria-hidden="true" />
      );
  }
}

export const AgentAvatar = memo(function AgentAvatar({
  actor,
  size = "sm",
  active = false,
  className,
}: {
  actor: string;
  size?: "xs" | "sm" | "md";
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
  const iconSize =
    size === "xs" ? "size-3" : size === "md" ? "size-3.5" : "size-3";

  return (
    <span
      data-slot="agent-avatar"
      data-actor={identity.actor}
      className={cn(
        "relative inline-flex shrink-0 items-center justify-center rounded-full ring-1 select-none transition-transform",
        boxSize,
        identity.bgClass,
        identity.textClass,
        identity.ringClass,
        className,
      )}
    >
      {renderRoleIcon(identity.iconKind, iconSize, identity.monogram)}
      {active && (
        <span
          aria-hidden="true"
          className={cn(
            "absolute -right-0.5 -bottom-0.5 size-2 rounded-full ring-2 ring-background animate-pulse",
            identity.dotClass,
          )}
        />
      )}
    </span>
  );
});

export { BookOpenIcon };
