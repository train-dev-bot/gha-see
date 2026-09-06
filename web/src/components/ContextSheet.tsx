import { useEffect, useMemo, useState } from "react";
import type { DispatchInputDto, EvalContextDto, WorkflowDto } from "../types";

/** Common GitHub Actions `on:` / `github.event_name` values. */
export const GITHUB_EVENTS = [
  "branch_protection_rule",
  "check_run",
  "check_suite",
  "create",
  "delete",
  "deployment",
  "deployment_status",
  "discussion",
  "discussion_comment",
  "fork",
  "gollum",
  "issue_comment",
  "issues",
  "label",
  "merge_group",
  "milestone",
  "page_build",
  "public",
  "pull_request",
  "pull_request_review",
  "pull_request_review_comment",
  "pull_request_target",
  "push",
  "registry_package",
  "release",
  "repository_dispatch",
  "schedule",
  "status",
  "watch",
  "workflow_call",
  "workflow_dispatch",
  "workflow_run",
] as const;

const EMPTY_DISPATCH_INPUTS: DispatchInputDto[] = [];

interface ContextSheetProps {
  context: EvalContextDto;
  /** Active workflow — used to hint declared dispatch inputs. */
  workflow?: WorkflowDto | null;
  onApply: (context: EvalContextDto) => void;
  applying: boolean;
}

function cloneContext(ctx: EvalContextDto): EvalContextDto {
  return structuredClone(ctx);
}

/** Parse key=value lines; keep incomplete lines out of the map but don't destroy typing. */
function parseInputsText(text: string): Record<string, string> {
  const inputs: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq > 0) {
      inputs[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
    }
  }
  return inputs;
}

export function inputsTextForWorkflow(
  declared: DispatchInputDto[],
  contextInputs: Record<string, string>,
): string {
  return declared
    .map((input) => {
      const value = contextInputs[input.name] ?? input.default ?? "";
      return `${input.name}=${value}`;
    })
    .join("\n");
}

export function mergeWorkflowInputs(
  all: Record<string, string>,
  declaredNames: string[],
  parsed: Record<string, string>,
): Record<string, string> {
  const next = { ...all };
  const declared = new Set(declaredNames);
  for (const name of declaredNames) {
    if (Object.prototype.hasOwnProperty.call(parsed, name)) {
      next[name] = parsed[name]!;
    } else {
      delete next[name];
    }
  }
  for (const [name, value] of Object.entries(parsed)) {
    if (!declared.has(name)) {
      next[name] = value;
    }
  }
  return next;
}

function eventOptions(current: string): string[] {
  if (GITHUB_EVENTS.includes(current as (typeof GITHUB_EVENTS)[number])) {
    return [...GITHUB_EVENTS];
  }
  return [current, ...GITHUB_EVENTS];
}

export default function ContextSheet({
  context,
  workflow,
  onApply,
  applying,
}: ContextSheetProps) {
  const [draft, setDraft] = useState<EvalContextDto>(() => cloneContext(context));
  const declaredInputs = workflow?.dispatchInputs ?? EMPTY_DISPATCH_INPUTS;
  const declaredNames = useMemo(
    () => declaredInputs.map((input) => input.name),
    [declaredInputs],
  );
  const [inputsText, setInputsText] = useState(() =>
    inputsTextForWorkflow(workflow?.dispatchInputs ?? [], context.inputs),
  );

  const workflowKey = workflow?.index ?? -1;

  useEffect(() => {
    setDraft(cloneContext(context));
    if (declaredInputs.length > 0) {
      setInputsText(inputsTextForWorkflow(declaredInputs, context.inputs));
    }
  }, [context, declaredInputs]);

  useEffect(() => {
    if (declaredInputs.length === 0) {
      setInputsText("");
    }
  }, [workflowKey, declaredInputs.length]);

  const updateField = <K extends keyof EvalContextDto>(
    key: K,
    value: EvalContextDto[K],
  ) => {
    setDraft((prev) => ({ ...prev, [key]: value }));
  };

  const updateGithub = (
    field: keyof EvalContextDto["github"],
    value: string,
  ) => {
    setDraft((prev) => ({
      ...prev,
      github: { ...prev.github, [field]: value },
    }));
  };

  const onInputsChange = (text: string) => {
    setInputsText(text);
    setDraft((prev) => ({ ...prev, inputs: parseInputsText(text) }));
  };

  const dirty = useMemo(() => {
    const parsed = parseInputsText(inputsText);
    const inputsDirty =
      declaredInputs.length > 0
        ? inputsText !== inputsTextForWorkflow(declaredInputs, context.inputs)
        : Object.entries(parsed).some(
            ([key, value]) => context.inputs[key] !== value,
          );
    return (
      draft.eventName !== context.eventName ||
      draft.refName !== context.refName ||
      draft.github.sha !== context.github.sha ||
      draft.github.repository !== context.github.repository ||
      draft.github.actor !== context.github.actor ||
      inputsDirty
    );
  }, [context, draft, inputsText, declaredInputs]);

  const handleReset = () => {
    setDraft(cloneContext(context));
    setInputsText(inputsTextForWorkflow(declaredInputs, context.inputs));
  };

  const handleApply = () => {
    const next = {
      ...draft,
      inputs: mergeWorkflowInputs(
        context.inputs,
        declaredNames,
        parseInputsText(inputsText),
      ),
    };
    setDraft(next);
    onApply(next);
  };

  const events = eventOptions(draft.eventName);

  return (
    <div className="context-sheet">
      <p className="context-help">
        Mock context for what-if on <code>if:</code> — edit event/ref/inputs,
        then <strong>Apply</strong>. Nothing executes.
      </p>

      <div className="context-sheet__fields">
        <label className="context-field">
          <span id="context-trigger-event-label">Trigger event</span>
          <select
            className="input context-select"
            aria-labelledby="context-trigger-event-label"
            value={draft.eventName}
            onChange={(e) => updateField("eventName", e.target.value)}
          >
            {events.map((ev) => (
              <option key={ev} value={ev}>
                {ev}
              </option>
            ))}
          </select>
          <span className="context-hint">
            Maps to <code>github.event_name</code>
          </span>
        </label>

        <label className="context-field">
          <span>Git ref</span>
          <input
            className="input mono"
            value={draft.refName}
            onChange={(e) => updateField("refName", e.target.value)}
            placeholder="refs/heads/main"
            spellCheck={false}
          />
          <span className="context-hint">
            Maps to <code>github.ref</code> (try <code>refs/heads/dev</code>)
          </span>
        </label>

        <fieldset className="context-group">
          <legend>github context</legend>
          <label className="context-field">
            <span>sha</span>
            <input
              className="input mono"
              value={draft.github.sha}
              onChange={(e) => updateGithub("sha", e.target.value)}
              spellCheck={false}
            />
          </label>
          <label className="context-field">
            <span>repository</span>
            <input
              className="input mono"
              value={draft.github.repository}
              onChange={(e) => updateGithub("repository", e.target.value)}
              spellCheck={false}
            />
          </label>
          <label className="context-field">
            <span>actor</span>
            <input
              className="input"
              value={draft.github.actor}
              onChange={(e) => updateGithub("actor", e.target.value)}
              spellCheck={false}
            />
          </label>
        </fieldset>

        <label className="context-field context-field--textarea">
          <span>Inputs (<code>inputs.*</code>)</span>
          <span className="context-hint context-hint--above">
            One <code>key=value</code> per line for{" "}
            <code>workflow_dispatch</code> / <code>workflow_call</code> inputs
            {workflow?.name ? (
              <>
                {" "}
                (active: <code>{workflow.name}</code>)
              </>
            ) : null}
            {declaredInputs.length > 0 ? (
              <>
                . Keys:{" "}
                {declaredInputs.map((input, i) => (
                  <span key={input.name}>
                    {i > 0 ? ", " : null}
                    <code>{input.name}</code>
                  </span>
                ))}
                .
              </>
            ) : (
              ". This workflow declares no dispatch/call inputs — you can still add key=value lines for what-if."
            )}
          </span>
          <textarea
            className="input context-textarea mono"
            rows={
              declaredInputs.length > 0
                ? Math.min(8, Math.max(4, declaredInputs.length + 1))
                : 3
            }
            value={inputsText}
            onChange={(e) => onInputsChange(e.target.value)}
            placeholder={"environment=production\nskip_dast=true"}
            spellCheck={false}
          />
        </label>
      </div>

      <div className="context-sheet__footer">
        {dirty && <span className="context-dirty">Unsaved changes</span>}
        <button
          type="button"
          className="btn"
          disabled={applying || !dirty}
          onClick={handleReset}
        >
          Reset
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={applying}
          onClick={handleApply}
        >
          {applying ? "Applying…" : "Apply"}
        </button>
      </div>
    </div>
  );
}
