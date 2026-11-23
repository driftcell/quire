import { memo, type ReactNode } from "react";
import { CopyIcon } from "lucide-react";
import { toast } from "sonner";
import { Streamdown } from "streamdown";
import { AgentAvatar } from "@/components/chat/agentAvatar";
import { HistoricalRunTrace } from "@/components/chat/ExecutionTraceSection";
import { UserMessageBubble } from "@/components/chat/UserMessageBubble";
import {
  CHAT_PROSE,
  STREAMDOWN_CONTROLS,
} from "@/components/chat/chatMarkdown";
import { Button } from "@/components/ui/button";
import { Message, MessageContent, MessageGroup } from "@/components/ui/message";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type {
  AttachmentMeta,
  RunTraceSummary,
  SessionMessage,
} from "@/contracts/session-protocol";

const timeFormatter = new Intl.DateTimeFormat("zh-CN", {
  month: "numeric",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
});

const fullTimeFormatter = new Intl.DateTimeFormat("zh-CN", {
  dateStyle: "medium",
  timeStyle: "medium",
});

function formatMsgTime(iso: string): { label: string; full: string } | null {
  const date = new Date(iso);
  if (!Number.isFinite(date.getTime())) return null;
  return {
    label: timeFormatter.format(date),
    full: fullTimeFormatter.format(date),
  };
}

async function copyText(text: string): Promise<void> {
  const clipboard = navigator.clipboard;
  if (clipboard && typeof clipboard.writeText === "function") {
    await clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  try {
    document.execCommand("copy");
  } finally {
    textarea.remove();
  }
}

function CopyMessageButton({ content }: { content: string }) {
  const handleCopy = async () => {
    try {
      await copyText(content);
      toast.success("已复制到剪贴板");
    } catch {
      toast.error("复制失败，请手动选择文本");
    }
  };

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label="拷贝回复"
            onClick={() => {
              void handleCopy();
            }}
            className="text-muted-foreground hover:text-foreground"
          />
        }
      >
        <CopyIcon />
      </TooltipTrigger>
      <TooltipContent>拷贝</TooltipContent>
    </Tooltip>
  );
}

export interface HistoricalMessageItemProps {
  sessionId: string;
  msg: SessionMessage;
  attachmentMap: Map<string, AttachmentMeta>;
  trace?: RunTraceSummary;
  questionSlots?: Map<string, ReactNode>;
  onJumpToSection?: (sectionId: string) => void;
}

export const HistoricalMessageItem = memo(function HistoricalMessageItem({
  sessionId,
  msg,
  attachmentMap,
  trace,
  questionSlots,
  onJumpToSection,
}: HistoricalMessageItemProps) {
  const msgAttachmentIds = msg.attachment_ids ?? [];
  const sentAt = formatMsgTime(msg.created_at);
  const userCommentRefs = msg.comment_refs ?? [];
  const hasTextSegment =
    trace?.segments?.some(
      (segment) => segment.kind === "text" && segment.text.trim().length > 0,
    ) ?? false;
  const textSegmentContent =
    trace?.segments
      ?.filter((segment) => segment.kind === "text")
      .map((segment) => segment.text)
      .join("") ?? "";
  const copyContent =
    msg.content.trim().length > 0 ? msg.content : textSegmentContent;

  if (msg.role === "user") {
    return (
      <MessageGroup className="group/user gap-1">
        {sentAt && (
          <time
            dateTime={msg.created_at}
            title={sentAt.full}
            className="self-end px-1 text-[10.5px] leading-none text-muted-foreground/60 tabular-nums opacity-0 transition-opacity group-hover/user:opacity-100"
          >
            {sentAt.label}
          </time>
        )}
        <UserMessageBubble
          body={msg.content}
          commentRefs={userCommentRefs}
          attachmentIds={msgAttachmentIds}
          attachmentMap={attachmentMap}
          onJumpToSection={onJumpToSection}
        />
      </MessageGroup>
    );
  }

  return (
    <MessageGroup className="group/assistant">
      <Message align="start" className="items-start gap-2.5">
        <AgentAvatar actor="main" size="sm" className="mt-0.5" />
        <MessageContent className="min-w-0 flex-1 gap-2">
          {trace && (
            <HistoricalRunTrace
              sessionId={sessionId}
              trace={trace}
              questionSlots={questionSlots}
            />
          )}
          {!hasTextSegment && msg.content.trim().length > 0 && (
            <div className={CHAT_PROSE}>
              <Streamdown
                mode="static"
                isAnimating={false}
                animated={false}
                controls={STREAMDOWN_CONTROLS}
              >
                {msg.content}
              </Streamdown>
            </div>
          )}
          <div className="flex w-full items-center gap-2 pt-0.5">
            {sentAt && (
              <time
                dateTime={msg.created_at}
                title={sentAt.full}
                className="text-[11px] text-muted-foreground/60 tabular-nums"
              >
                {sentAt.label}
              </time>
            )}
            <div className="ml-auto flex items-center gap-1 opacity-70 transition-opacity group-hover/assistant:opacity-100">
              <CopyMessageButton content={copyContent} />
            </div>
          </div>
        </MessageContent>
      </Message>
    </MessageGroup>
  );
});
