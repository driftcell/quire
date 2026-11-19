import { useState } from "react";
import {
  CheckCircle2Icon,
  CheckIcon,
  HelpCircleIcon,
  SkipForwardIcon,
} from "lucide-react";
import { cn } from "cn";
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

function hasAnswer(
  question: QuestionItem,
  selectedByQ: Record<string, string[]>,
  freeformByQ: Record<string, string>,
) {
  return (
    (selectedByQ[question.id]?.length ?? 0) > 0 ||
    (freeformByQ[question.id]?.trim().length ?? 0) > 0
  );
}

function QuestionNav({
  questions,
  activeId,
  selectedByQ,
  freeformByQ,
  onSelect,
}: {
  questions: QuestionItem[];
  activeId: string;
  selectedByQ: Record<string, string[]>;
  freeformByQ: Record<string, string>;
  onSelect: (id: string) => void;
}) {
  return (
    <nav
      aria-label="问题导航"
      className="mb-4 flex flex-wrap items-center gap-1"
    >
      {questions.map((question, index) => {
        const active = question.id === activeId;
        const answered = hasAnswer(question, selectedByQ, freeformByQ);
        return (
          <button
            key={question.id}
            type="button"
            onClick={() => onSelect(question.id)}
            aria-current={active ? "step" : undefined}
            aria-label={`问题 ${index + 1}：${question.title}`}
            title={question.title}
            className={cn(
              "inline-flex min-w-0 items-center gap-1.5 rounded-md px-2 py-1 text-xs transition-colors",
              active
                ? "bg-muted text-foreground"
                : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
            )}
          >
            <span className="tabular-nums">{index + 1}</span>
            <span className="max-w-40 truncate">{question.title}</span>
            {answered && <CheckIcon className="size-3 shrink-0 text-primary" />}
          </button>
        );
      })}
    </nav>
  );
}

function QuestionReview({
  questions,
  answers,
  status,
}: {
  questions: QuestionItem[];
  answers: Map<string, QuestionAnswer>;
  status: "answered" | "skipped" | "dismissed";
}) {
  return (
    <div className="flex flex-col">
      {questions.map((question, index) => {
        const answer = answers.get(question.id);
        const selected = (answer?.selected_option_ids ?? []).map((id) => ({
          id,
          label:
            question.options?.find((option) => option.id === id)?.label ?? id,
        }));
        const freeform = answer?.freeform_text?.trim() ?? "";
        const skipped = answer?.skipped ?? status === "skipped";
        return (
          <section
            key={question.id}
            className={cn(index > 0 && "mt-5 border-t border-border/60 pt-5")}
          >
            <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
              <span className="text-[11px] font-medium text-muted-foreground/70 tabular-nums">
                问题 {index + 1}
              </span>
              <h3 className="text-sm font-medium text-foreground">
                {question.title}
              </h3>
            </div>
            {question.description && (
              <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                {question.description}
              </p>
            )}
            <div className="mt-2.5">
              {selected.length > 0 && (
                <ul className="flex flex-col gap-1">
                  {selected.map((option) => (
                    <li
                      key={option.id}
                      className="flex items-start gap-2 text-sm text-foreground"
                    >
                      <CheckIcon className="mt-0.5 size-3.5 shrink-0 text-primary" />
                      <span>{option.label}</span>
                    </li>
                  ))}
                </ul>
              )}
              {freeform.length > 0 && (
                <p
                  className={cn(
                    "border-l-2 border-border pl-3 text-sm leading-relaxed whitespace-pre-wrap text-foreground/80",
                    selected.length > 0 && "mt-2",
                  )}
                >
                  {freeform}
                </p>
              )}
              {selected.length === 0 && freeform.length === 0 && (
                <p className="text-sm text-muted-foreground">
                  {skipped ? "已跳过" : "未回答"}
                </p>
              )}
            </div>
          </section>
        );
      })}
    </div>
  );
}

export function QuestionCard({
  pending,
  resolvedState,
  isSubmitting,
  onSubmit,
}: QuestionCardProps) {
  const [selectedByQ, setSelectedByQ] = useState<Record<string, string[]>>({});
  const [freeformByQ, setFreeformByQ] = useState<Record<string, string>>({});
  const [activeQuestionId, setActiveQuestionId] = useState(
    () => pending.questions[0]?.id ?? "",
  );

  const isFrozen = resolvedState !== undefined;
  const answerByQ = new Map<string, QuestionAnswer>();
  if (resolvedState?.answers) {
    for (const answer of resolvedState.answers) {
      answerByQ.set(answer.question_id, answer);
    }
  }

  const toggleOption = (question: QuestionItem, optionId: string) => {
    if (isFrozen) return;
    setSelectedByQ((prev) => {
      const current = prev[question.id] ?? [];
      if (!question.multiple) {
        return { ...prev, [question.id]: [optionId] };
      }
      const exists = current.includes(optionId);
      return {
        ...prev,
        [question.id]: exists
          ? current.filter((id) => id !== optionId)
          : [...current, optionId],
      };
    });
  };

  const handleSubmit = () => {
    if (isFrozen) return;
    const answers: QuestionAnswer[] = pending.questions.map((question) => {
      const selected = selectedByQ[question.id] ?? [];
      const freeform = (freeformByQ[question.id] ?? "").trim();
      const hasContent = selected.length > 0 || freeform.length > 0;
      return {
        question_id: question.id,
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
        </CardTitle>
      </CardHeader>
      <CardContent>
        {isFrozen ? (
          <QuestionReview
            questions={pending.questions}
            answers={answerByQ}
            status={resolvedState.status}
          />
        ) : (
          <>
            {pending.questions.length > 1 && (
              <QuestionNav
                questions={pending.questions}
                activeId={activeQuestionId}
                selectedByQ={selectedByQ}
                freeformByQ={freeformByQ}
                onSelect={setActiveQuestionId}
              />
            )}
            <Questionnaire
              item={activeQuestionId || undefined}
              onItemChange={setActiveQuestionId}
              onSubmit={(event) => {
                event.preventDefault();
                handleSubmit();
              }}
            >
              {pending.questions.map((question) => {
                const selected = selectedByQ[question.id] ?? [];
                const freeform = freeformByQ[question.id] ?? "";
                const options = question.options ?? [];
                return (
                  <QuestionnaireItem
                    key={question.id}
                    name={question.id}
                    multiple={question.multiple}
                  >
                    <QuestionnaireTitle>{question.title}</QuestionnaireTitle>
                    {question.description && (
                      <QuestionnaireDescription>
                        {question.description}
                      </QuestionnaireDescription>
                    )}
                    {options.length > 0 && (
                      <QuestionnaireChoices>
                        {options.map((option) => (
                          <QuestionnaireChoice
                            key={option.id}
                            value={option.id}
                            checked={selected.includes(option.id)}
                            onChange={() => toggleOption(question, option.id)}
                          >
                            <span>{option.label}</span>
                            {option.description && (
                              <QuestionnaireChoiceDescription>
                                {option.description}
                              </QuestionnaireChoiceDescription>
                            )}
                          </QuestionnaireChoice>
                        ))}
                      </QuestionnaireChoices>
                    )}
                    <QuestionnaireInput
                      aria-label={`${question.title} 补充说明`}
                      placeholder="自由补充说明（可选）..."
                      value={freeform}
                      onChange={(event) => {
                        const value = event.target.value;
                        setFreeformByQ((prev) => ({
                          ...prev,
                          [question.id]: value,
                        }));
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
          </>
        )}
      </CardContent>
    </Card>
  );
}
