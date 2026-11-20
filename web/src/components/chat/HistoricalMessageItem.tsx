import { memo, useState, type ReactNode } from "react";
import { CheckIcon, CopyIcon } from "lucide-react";
import { Streamdown } from "streamdown";
import { HistoricalRunTrace } from "@/components/chat/ExecutionTraceSection";
import { MessageAttachments } from "@/components/chat/MessageAttachments";
import { UserCommentRefCards } from "@/components/chat/PendingUserBubble";
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
import { parseCommentRefs } from "@/lib/commentRefXml";

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
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await copyText(content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
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
            aria-label={copied ? "已复制" : "拷贝回复"}
            onClick={() => {
              void handleCopy();
            }}
            className="text-muted-foreground hover:text-foreground"
          />
        }
      >
        {copied ? (
          <CheckIcon className="size-3.5 text-emerald-600 dark:text-emerald-400" />
        ) : (
          <CopyIcon className="size-3.5" />
        )}
      </TooltipTrigger>
      <TooltipContent>{copied ? "已复制" : "拷贝"}</TooltipContent>
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
  const parsedUserContent =
    msg.role === "user" ? parseCommentRefs(msg.content) : null;
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
    const userRefs = parsedUserContent?.refs ?? [];
    const userBody = parsedUserContent?.body ?? msg.content;
    return (
      <MessageGroup className="gap-1">
        {sentAt && (
          <time
            dateTime={msg.created_at}
            title={sentAt.full}
            className="self-end px-1 text-[11px] leading-none text-muted-foreground/70 tabular-nums"
          >
            {sentAt.label}
          </time>
        )}
        <Message align="end" className="items-end">
          <div className="flex max-w-[85%] flex-col items-end">
            {userRefs.length > 0 && (
              <div className="w-full">
                <UserCommentRefCards
                  refs={userRefs}
                  onJumpToSection={onJumpToSection}
                />
              </div>
            )}
            <MessageAttachments
              ids={msgAttachmentIds}
              attachmentMap={attachmentMap}
            />
            {userBody.length > 0 && (
              <div className="w-fit max-w-full rounded-2xl bg-muted px-4 py-2.5 text-sm leading-6 whitespace-pre-wrap text-foreground">
                {userBody}
              </div>
            )}
          </div>
        </Message>
      </MessageGroup>
    );
  }

  return (
    <MessageGroup className="group/assistant">
      <Message align="start" className="flex-col items-start">
        {trace && (
          <HistoricalRunTrace
            sessionId={sessionId}
            trace={trace}
            questionSlots={questionSlots}
          />
        )}
        {!hasTextSegment && (
          <MessageContent className={CHAT_PROSE}>
            <Streamdown controls={STREAMDOWN_CONTROLS}>
              {msg.content}
            </Streamdown>
          </MessageContent>
        )}
        <div className="-mt-1 flex w-full items-center gap-2">
          {sentAt && (
            <time
              dateTime={msg.created_at}
              title={sentAt.full}
              className="px-1 text-[11px] text-muted-foreground/60 tabular-nums"
            >
              {sentAt.label}
            </time>
          )}
          <div className="ml-auto flex items-center gap-1">
            <CopyMessageButton content={copyContent} />
          </div>
        </div>
      </Message>
    </MessageGroup>
  );
});
