import type { ReactNode } from "react";
import { extractJobYaml, extractStepYaml } from "../extractDefinition";
import type { RunState, Selection, WebView } from "../types";
import {
  RUN_STATE_COLORS,
  RUN_STATE_LABELS,
  jobState,
  severityClass,
  stepState,
} from "../types";

interface InspectorPaneProps {
  view: WebView;
  selection: Selection | null;
  onFetchWorkflow?: (index: number) => void;
  fetchingWorkflowIndex?: number | null;
}

const SKIPPED_HINT =
  "Skipped by if: / upstream conditions (not counted as issues)";

function RunStateValue({ state }: { state: RunState }) {
  return (
    <dd>
      <span style={{ color: RUN_STATE_COLORS[state] }}>
        {RUN_STATE_LABELS[state]}
      </span>
      {state === "skipped" && (
        <span className="inspector-run-hint">{SKIPPED_HINT}</span>
      )}
    </dd>
  );
}

function relevantFindings(view: WebView, selection: Selection | null) {
  if (!selection) return view.findings.slice(0, 20);

  return view.findings.filter((f) => {
    const t = f.target;
    if (selection.kind === "workflow") {
      const wf = view.workflows.find((w) => w.index === selection.workflowIndex);
      if (!wf) return false;
      return t.file === wf.path;
    }
    if (selection.kind === "job") {
      return (
        (t.kind === "job" || t.kind === "step") &&
        t.file === selection.file &&
        t.job === selection.instanceId
      );
    }
    return (
      t.kind === "step" &&
      t.file === selection.file &&
      t.job === selection.instanceId &&
      t.stepIdx === selection.stepIdx
    );
  });
}

export default function InspectorPane({
  view,
  selection,
  onFetchWorkflow,
  fetchingWorkflowIndex = null,
}: InspectorPaneProps) {
  const findings = relevantFindings(view, selection);

  const selectedWorkflow =
    selection != null
      ? view.workflows.find((w) => w.index === selection.workflowIndex) ?? null
      : null;
  const showFetch =
    selectedWorkflow?.needsRemoteFetch === true && onFetchWorkflow != null;

  let title = "Overview";
  let details: ReactNode = null;
  let definitionYaml: string | null = null;
  let definitionHint = "No source available";
  let copyText: string | null = null;
  let copyLabel = "Copy";

  if (selection?.kind === "workflow") {
    const wf = view.workflows.find((w) => w.index === selection.workflowIndex);
    if (wf) {
      title = wf.name ?? wf.path;
      definitionYaml = wf.rawSource;
      details = (
        <dl className="inspector-dl">
          <dt>Path</dt>
          <dd className="mono">{wf.path}</dd>
          <dt>Parse</dt>
          <dd>{wf.parseOk ? "OK" : "Failed"}</dd>
          {wf.fileError && (
            <>
              <dt>Error</dt>
              <dd className="finding-error">{wf.fileError}</dd>
            </>
          )}
          <dt>Jobs</dt>
          <dd>{wf.instances.length}</dd>
          <dt>Remote uses</dt>
          <dd>
            {wf.hasRemoteUses
              ? wf.needsRemoteFetch
                ? "Unresolved (Fetch available)"
                : "Resolved from cache"
              : "No"}
          </dd>
        </dl>
      );
    }
  } else if (selection?.kind === "job") {
    definitionHint = "Couldn’t extract this job";
    const wf = view.workflows.find((w) => w.index === selection.workflowIndex);
    const inst = wf?.instances.find((i) => i.instanceId === selection.instanceId);
    if (inst && wf) {
      title = inst.instanceId;
      definitionYaml = wf.rawSource
        ? extractJobYaml(wf.rawSource, inst.baseId)
        : null;
      const state = jobState(view, selection.file, selection.instanceId);
      details = (
        <dl className="inspector-dl">
          <dt>Base job</dt>
          <dd>{inst.baseId}</dd>
          <dt>Run state</dt>
          <RunStateValue state={state} />
          <dt>Runs on</dt>
          <dd>{inst.runsOn ?? "—"}</dd>
          <dt>Support</dt>
          <dd>{inst.support}</dd>
          {inst.condition && (
            <>
              <dt>Condition</dt>
              <dd className="mono inspector-condition">{inst.condition}</dd>
            </>
          )}
          {inst.needs.length > 0 && (
            <>
              <dt>Needs</dt>
              <dd>{inst.needs.join(", ")}</dd>
            </>
          )}
          {Object.keys(inst.matrix).length > 0 && (
            <>
              <dt>Matrix</dt>
              <dd className="mono">
                {Object.entries(inst.matrix)
                  .map(([k, v]) => `${k}=${v}`)
                  .join(", ")}
              </dd>
            </>
          )}
          {Object.keys(inst.outputs).length > 0 && (
            <>
              <dt>Outputs</dt>
              <dd className="mono">
                {Object.entries(inst.outputs)
                  .map(([k, v]) => `${k}: ${v}`)
                  .join("\n")}
              </dd>
            </>
          )}
          {inst.environment && (
            <>
              <dt>Environment</dt>
              <dd>
                {inst.environment.name}
                {inst.environment.url ? (
                  <span className="mono"> ({inst.environment.url})</span>
                ) : null}
              </dd>
            </>
          )}
          {inst.services.length > 0 && (
            <>
              <dt>Services</dt>
              <dd className="mono">
                {inst.services
                  .map((s) => (s.image ? `${s.name}: ${s.image}` : s.name))
                  .join("\n")}
              </dd>
            </>
          )}
          {inst.concurrency && (
            <>
              <dt>Concurrency</dt>
              <dd className="mono">
                {inst.concurrency.group}
                {inst.concurrency.cancelInProgress != null
                  ? ` (cancel-in-progress: ${inst.concurrency.cancelInProgress})`
                  : ""}
              </dd>
            </>
          )}
        </dl>
      );
    }
  } else if (selection?.kind === "step") {
    definitionHint = "Couldn’t extract this step";
    const wf = view.workflows.find((w) => w.index === selection.workflowIndex);
    const inst = wf?.instances.find((i) => i.instanceId === selection.instanceId);
    const step = inst?.steps.find((s) => s.index === selection.stepIdx);
    if (step && inst && wf) {
      title = step.name ?? step.uses ?? step.run ?? `Step ${step.index}`;
      definitionYaml = wf.rawSource
        ? extractStepYaml(wf.rawSource, inst.baseId, selection.stepIdx)
        : null;
      copyText = step.uses ?? step.run;
      copyLabel = step.uses ? "Copy uses" : "Copy run";
      const state = stepState(
        view,
        selection.file,
        selection.instanceId,
        selection.stepIdx,
      );
      details = (
        <dl className="inspector-dl">
          <dt>Job</dt>
          <dd>{selection.instanceId}</dd>
          <dt>Index</dt>
          <dd>{step.index}</dd>
          <dt>Run state</dt>
          <RunStateValue state={state} />
          {step.id && (
            <>
              <dt>ID</dt>
              <dd className="mono">{step.id}</dd>
            </>
          )}
          {step.uses && (
            <>
              <dt>Uses</dt>
              <dd className="mono">{step.uses}</dd>
            </>
          )}
          {step.resolvedActionName && (
            <>
              <dt>Resolved</dt>
              <dd className="mono">{step.resolvedActionName}</dd>
            </>
          )}
          {step.actionRunner && (
            <>
              <dt>Runner</dt>
              <dd className="mono">{step.actionRunner}</dd>
            </>
          )}
          {step.run && (
            <>
              <dt>Run</dt>
              <dd className="mono inspector-run">
                {step.run.length > 400 ? `${step.run.slice(0, 400)}…` : step.run}
              </dd>
            </>
          )}
          {step.shell && (
            <>
              <dt>Shell</dt>
              <dd className="mono">{step.shell}</dd>
            </>
          )}
          {step.workingDirectory && (
            <>
              <dt>Working dir</dt>
              <dd className="mono">{step.workingDirectory}</dd>
            </>
          )}
          {Object.keys(step.withInputs).length > 0 && (
            <>
              <dt>With</dt>
              <dd className="mono inspector-run">
                {Object.entries(step.withInputs)
                  .map(([k, v]) => `${k}: ${v}`)
                  .join("\n")}
              </dd>
            </>
          )}
          {Object.keys(step.env).length > 0 && (
            <>
              <dt>Env</dt>
              <dd className="mono inspector-run">
                {Object.entries(step.env)
                  .map(([k, v]) => `${k}=${v}`)
                  .join("\n")}
              </dd>
            </>
          )}
          {step.compositeStepCount > 0 && (
            <>
              <dt>Composite</dt>
              <dd>{step.compositeStepCount} nested steps</dd>
            </>
          )}
          {step.condition && (
            <>
              <dt>Condition</dt>
              <dd className="mono inspector-condition">{step.condition}</dd>
            </>
          )}
        </dl>
      );
    }
  } else {
    title = "Nothing selected";
    details = (
      <p className="inspector-hint">
        Select a workflow, job, or step from the tree or graph to see its
        run state, conditions, inputs, and findings.
      </p>
    );
  }

  const bindings =
    selection?.kind === "job"
      ? view.bindings.filter(
          (b) =>
            b.file === selection.file &&
            (b.consumerJob === selection.instanceId ||
              b.producerJob === selection.instanceId),
        )
      : [];

  return (
    <div className="inspector">
      <div className="inspector__details">
        <div className="inspector__title-row">
          <h3 className="inspector__title">{title}</h3>
          <div className="inspector__title-actions">
            {showFetch && selectedWorkflow && (
              <button
                type="button"
                className="btn btn-fetch"
                disabled={fetchingWorkflowIndex === selectedWorkflow.index}
                onClick={() => onFetchWorkflow(selectedWorkflow.index)}
                title="Download missing remote actions for this workflow"
              >
                {fetchingWorkflowIndex === selectedWorkflow.index
                  ? "Fetching…"
                  : "Fetch remotes"}
              </button>
            )}
            {copyText && (
              <button
                type="button"
                className="btn inspector__copy"
                onClick={() => void navigator.clipboard.writeText(copyText)}
              >
                {copyLabel}
              </button>
            )}
          </div>
        </div>
        {details}

        {bindings.length > 0 && (
          <section className="inspector-section">
            <h4>Dataflow bindings</h4>
            <ul className="inspector-bindings">
              {bindings.map((b, i) => (
                <li key={i} className="mono">
                  {b.producerJob} → {b.consumerJob} ({b.outputName})
                </li>
              ))}
            </ul>
          </section>
        )}

        {findings.length > 0 && (
          <section className="inspector-section">
            <h4>Issues ({findings.length})</h4>
            <ul className="inspector-findings">
              {findings.map((f, i) => (
                <li key={i} className={severityClass(f.severity)}>
                  <span className="finding-code">[{f.code}]</span> {f.message}
                  {f.coach && <div className="finding-coach">{f.coach}</div>}
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>

      {selection != null && (
        <div className="inspector__yaml">
          <h4>This definition</h4>
          {definitionYaml ? (
            <pre className="yaml-block">{definitionYaml}</pre>
          ) : (
            <p className="inspector-hint">{definitionHint}</p>
          )}
        </div>
      )}
    </div>
  );
}
