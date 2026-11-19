import { useState } from "react";
import {
  CheckCircle2Icon,
  HelpCircleIcon,
  SkipForwardIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Questionnaire,
  QuestionnaireActions,
  QuestionnaireChoice,
  QuestionnaireChoiceDescription,
  QuestionnaireChoices,
  QuestionnaireDescription,
  QuestionnaireInput,
  QuestionnaireItem,
  QuestionnaireNext,
  QuestionnairePrevious,
  QuestionnaireProgress,
  QuestionnaireSkip,
  QuestionnaireSubmit,
  QuestionnaireTitle,
} from "@/components/ui/questionnaire";
import type {
  PendingQuestion,
  QuestionAnswer,
  QuestionItem,
} from "@/contracts/session-protocol";

export interface QuestionCardProps {
  pending: PendingQuestion;
  resolvedState?: {
    status: "answered" | "skipped" | "dismissed";
    answers?: QuestionAnswer[];
  };
  isSubmitting: boolean;
  onSubmit: (answers: QuestionAnswer[], skipped: boolean) => void;
}

export function QuestionCard({
  pending,
  resolvedState,
  isSubmitting,
  onSubmit,
}: QuestionCardProps) {
  const [selectedByQ, setSelectedByQ] = useState<Record<string, string[]>>({});
  const [freeformByQ, setFreeformByQ] = useState<Record<string, string>>({});

  const isFrozen = resolvedState !== undefined;
  const answerByQ = new Map<string, QuestionAnswer>();
  if (resolvedState?.answers) {
    for (const answer of resolvedState.answers) {
      answerByQ.set(answer.question_id, answer);
    }
  }

  const toggleOption = (q: QuestionItem, optId: string) => {
    if (isFrozen) return;
    setSelectedByQ((prev) => {
      const current = prev[q.id] ?? [];
      if (!q.multiple) {
        return { ...prev, [q.id]: [optId] };
      }
      const exists = current.includes(optId);
      return {
        ...prev,
        [q.id]: exists
          ? current.filter((id) => id !== optId)
          : [...current, optId],
      };
    });
  };

  const handleSubmit = () => {
    if (isFrozen) return;
    const answers: QuestionAnswer[] = pending.questions.map((q) => {
      const selected = selectedByQ[q.id] ?? [];
      const freeform = (freeformByQ[q.id] ?? "").trim();
      const hasContent = selected.length > 0 || freeform.length > 0;
      return {
        question_id: q.id,
        selected_option_ids: selected,
        freeform_text: freeform.length > 0 ? freeform : undefined,
        skipped: !hasContent,
      };
    });
    onSubmit(answers, false);
  };

  return (
    <Card
      className={
        isFrozen
          ? "my-3 border-border/70 bg-muted/25 shadow-none"
          : "my-3 border-primary/25 shadow-sm"
      }
    >
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          {resolvedState?.status === "answered" ? (
            <CheckCircle2Icon className="size-4 text-emerald-600 dark:text-emerald-400" />
          ) : resolvedState ? (
            <SkipForwardIcon className="size-4 text-muted-foreground" />
          ) : (
            <HelpCircleIcon className="size-4 text-primary" />
          )}
          <span>
            {resolvedState?.status === "answered"
              ? "已提交问题回答"
              : resolvedState?.status === "skipped"
                ? "已跳过问题"
                : resolvedState?.status === "dismissed"
                  ? "已忽略问题"
                  : "Agent 需要你的确认"}
          </span>
          <Badge
            variant={isFrozen ? "outline" : "secondary"}
            className="ml-auto text-[11px]"
          >
            {isFrozen ? "已锁定" : "可跳过或直接输入新消息"}
          </Badge>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <Questionnaire
          onSubmit={(event) => {
            event.preventDefault();
            handleSubmit();
          }}
        >
          {pending.questions.length > 1 && <QuestionnaireProgress />}
          {pending.questions.map((q) => {
            const resolvedAnswer = answerByQ.get(q.id);
            const selected = isFrozen
              ? (resolvedAnswer?.selected_option_ids ?? selectedByQ[q.id] ?? [])
              : (selectedByQ[q.id] ?? []);
            const freeform = isFrozen
              ? (resolvedAnswer?.freeform_text ?? freeformByQ[q.id] ?? "")
              : (freeformByQ[q.id] ?? "");
            const options = q.options ?? [];
            return (
              <QuestionnaireItem key={q.id} name={q.id} multiple={q.multiple}>
                <QuestionnaireTitle>{q.title}</QuestionnaireTitle>
                {q.description && (
                  <QuestionnaireDescription>
                    {q.description}
                  </QuestionnaireDescription>
                )}
                {options.length > 0 && (
                  <QuestionnaireChoices>
                    {options.map((opt) => (
                      <QuestionnaireChoice
                        key={opt.id}
                        value={opt.id}
                        disabled={isFrozen}
                        checked={selected.includes(opt.id)}
                        onChange={() => toggleOption(q, opt.id)}
                      >
                        <span>{opt.label}</span>
                        {opt.description && (
                          <QuestionnaireChoiceDescription>
                            {opt.description}
                          </QuestionnaireChoiceDescription>
                        )}
                      </QuestionnaireChoice>
                    ))}
                  </QuestionnaireChoices>
                )}
                {(!isFrozen || freeform.trim().length > 0) && (
                  <QuestionnaireInput
                    aria-label={`${q.title} 补充说明`}
                    placeholder="自由补充说明（可选）..."
                    disabled={isFrozen}
                    readOnly={isFrozen}
                    value={freeform}
                    onChange={(event) => {
                      if (isFrozen) return;
                      const value = event.target.value;
                      setFreeformByQ((prev) => ({ ...prev, [q.id]: value }));
                    }}
                  />
                )}
                {!isFrozen && (
                  <QuestionnaireActions>
                    <QuestionnairePrevious />
                    <QuestionnaireSkip
                      type="button"
                      disabled={isSubmitting}
                      onClick={() => onSubmit([], true)}
                    >
                      跳过全部
                    </QuestionnaireSkip>
                    <QuestionnaireNext />
                    <QuestionnaireSubmit disabled={isSubmitting}>
                      {isSubmitting ? "提交中..." : "提交回答"}
                    </QuestionnaireSubmit>
                  </QuestionnaireActions>
                )}
              </QuestionnaireItem>
            );
          })}
        </Questionnaire>
      </CardContent>
    </Card>
  );
}
