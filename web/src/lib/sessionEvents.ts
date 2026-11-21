import type { SessionEvent } from "@/contracts/session-protocol";

export type SessionEventEnvelope = Pick<
  SessionEvent,
  "agent_id" | "parent_run_id"
>;

/** Mirror of the runtime's subagent envelope classification. */
export function isSubagentScope(event: SessionEventEnvelope): boolean {
  return (
    Boolean(event.parent_run_id) ||
    (event.agent_id !== "main" && event.agent_id.length > 0)
  );
}

/** Subagent usage belongs to the child run, not to the session context meter. */
export function shouldApplySessionUsage(event: SessionEventEnvelope): boolean {
  return !isSubagentScope(event);
}

/**
 * A terminal event starts an async refetch. Its delayed cleanup may run only if
 * no newer run has taken ownership of the live pane in the meantime.
 */
export function shouldResetLiveState(
  ownerRunId: string | null,
  terminalRunId: string,
): boolean {
  return ownerRunId === null || ownerRunId === terminalRunId;
}
