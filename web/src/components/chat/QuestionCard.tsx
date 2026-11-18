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

  if (resolvedState) {
    return (
      <Card className="my-2 border-dashed bg-muted/30">
        <CardHeader className="flex flex-row items-center justify-between space-y-0 py-3">
          <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
            {resolvedState.status === "answered" ? (
              <CheckCircle2Icon className="size-4 text-emerald-600" />
            ) : (
              <SkipForwardIcon className="size-4" />
            )}
            <span>
              {resolvedState.status === "answered"
                ? "已提交问题回答"
                : resolvedState.status === "skipped"
                  ? "已跳过问题卡片"
                  : "已发送新消息，原问题卡片已忽略"}
            </span>
          </div>
          <Badge variant="outline" className="text-[11px]">
            {pending.questions.length} 题
          </Badge>
        </CardHeader>
      </Card>
    );
  }

  const toggleOption = (q: QuestionItem, optId: string) => {
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
    <Card className="my-3 border-primary/25 shadow-sm">
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <HelpCircleIcon className="size-4 text-primary" />
          <span>Agent 需要你的确认</span>
          <Badge variant="secondary" className="ml-auto text-[11px]">
            可跳过或直接输入新消息
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
            const selected = selectedByQ[q.id] ?? [];
            const freeform = freeformByQ[q.id] ?? "";
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
                <QuestionnaireInput
                  aria-label={`${q.title} 补充说明`}
                  placeholder="自由补充说明（可选）..."
                  value={freeform}
                  onChange={(event) => {
                    const value = event.target.value;
                    setFreeformByQ((prev) => ({ ...prev, [q.id]: value }));
                  }}
                />
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
              </QuestionnaireItem>
            );
          })}
        </Questionnaire>
      </CardContent>
    </Card>
  );
}
