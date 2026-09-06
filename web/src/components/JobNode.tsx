import { memo, useCallback, useState, type MouseEvent } from "react";
import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import {
  RUN_STATE_COLORS,
  RUN_STATE_LABELS,
  stepKind,
  stepSubtitle,
  stepTitle,
  type JobNodeData,
  type StepDto,
} from "../types";

type JobFlowNode = Node<JobNodeData, "job">;

const MAX_COLLAPSED = 6;

function KindLabel({ step }: { step: StepDto }) {
  const kind = stepKind(step);
  const label =
    kind === "uses"
      ? "USES"
      : kind === "run"
        ? "RUN"
        : kind === "action"
          ? "ACTION"
          : "STEP";
  return <span className={`step-card__kind step-card__kind--${kind}`}>{label}</span>;
}

function ParamChips({
  entries,
  limit,
}: {
  entries: Record<string, string>;
  limit: number;
}) {
  const pairs = Object.entries(entries);
  if (pairs.length === 0) return null;
  const shown = pairs.slice(0, limit);
  const extra = pairs.length - shown.length;
  return (
    <div className="step-card__chips">
      {shown.map(([k, v]) => (
        <span key={k} className="step-chip" title={`${k}: ${v}`}>
          <span className="step-chip__k">{k}</span>
          <span className="step-chip__v">{truncate(v, 28)}</span>
        </span>
      ))}
      {extra > 0 && <span className="step-chip step-chip--more">+{extra}</span>}
    </div>
  );
}

function truncate(s: string, n: number): string {
  const one = s.replace(/\s+/g, " ").trim();
  return one.length > n ? `${one.slice(0, n)}…` : one;
}

function StepCard({
  step,
  state,
  selected,
  expanded,
  dimmed,
  onToggle,
  onSelect,
}: {
  step: StepDto;
  state: string;
  selected: boolean;
  expanded: boolean;
  dimmed: boolean;
  onToggle: () => void;
  onSelect: () => void;
}) {
  const subtitle = stepSubtitle(step);
  const withCount = Object.keys(step.withInputs).length;
  const envCount = Object.keys(step.env).length;

  const onClick = (e: MouseEvent) => {
    if (dimmed) return;
    e.stopPropagation();
    onSelect();
    onToggle();
  };

  return (
    <button
      type="button"
      className={`step-card${expanded ? " step-card--expanded" : ""}${
        selected ? " step-card--selected" : ""
      }`}
      style={{ borderLeftColor: RUN_STATE_COLORS[state as keyof typeof RUN_STATE_COLORS] }}
      onClick={onClick}
    >
      <div className="step-card__top">
        <span className="step-card__index">{step.index + 1}</span>
        <div className="step-card__titles">
          <span className="step-card__title">{stepTitle(step)}</span>
          {subtitle && <span className="step-card__sub mono">{subtitle}</span>}
        </div>
        <KindLabel step={step} />
      </div>

      {!expanded && (
        <>
          {step.uses && !step.name && (
            <div className="step-card__preview mono">{truncate(step.uses, 52)}</div>
          )}
          {step.run && (
            <pre className="step-card__code">{truncate(step.run, 72)}</pre>
          )}
          <ParamChips entries={step.withInputs} limit={3} />
          {(step.condition || step.compositeStepCount > 0 || envCount > 0) && (
            <div className="step-card__meta-row">
              {step.condition && <span className="step-pill">if</span>}
              {envCount > 0 && <span className="step-pill">env×{envCount}</span>}
              {withCount > 3 && <span className="step-pill">with×{withCount}</span>}
              {step.compositeStepCount > 0 && (
                <span className="step-pill">composite×{step.compositeStepCount}</span>
              )}
            </div>
          )}
        </>
      )}

      {expanded && (
        <div className="step-card__body">
          {step.id && (
            <div className="step-card__field">
              <span className="step-card__label">id</span>
              <code>{step.id}</code>
            </div>
          )}
          {step.uses && (
            <div className="step-card__field">
              <span className="step-card__label">uses</span>
              <code>{step.uses}</code>
            </div>
          )}
          {step.resolvedActionName && (
            <div className="step-card__field">
              <span className="step-card__label">resolved</span>
              <code>{step.resolvedActionName}</code>
            </div>
          )}
          {step.actionRunner && (
            <div className="step-card__field">
              <span className="step-card__label">runner</span>
              <code>{step.actionRunner}</code>
            </div>
          )}
          {step.run && (
            <div className="step-card__field">
              <span className="step-card__label">run</span>
              <pre className="step-card__code step-card__code--full">{step.run}</pre>
            </div>
          )}
          {step.shell && (
            <div className="step-card__field">
              <span className="step-card__label">shell</span>
              <code>{step.shell}</code>
            </div>
          )}
          {step.workingDirectory && (
            <div className="step-card__field">
              <span className="step-card__label">cwd</span>
              <code>{step.workingDirectory}</code>
            </div>
          )}
          {step.condition && (
            <div className="step-card__field">
              <span className="step-card__label">if</span>
              <code>{step.condition}</code>
            </div>
          )}
          {withCount > 0 && (
            <div className="step-card__field">
              <span className="step-card__label">with</span>
              <ParamChips entries={step.withInputs} limit={12} />
            </div>
          )}
          {envCount > 0 && (
            <div className="step-card__field">
              <span className="step-card__label">env</span>
              <ParamChips entries={step.env} limit={12} />
            </div>
          )}
          {step.compositeStepCount > 0 && (
            <div className="step-card__field">
              <span className="step-card__label">composite</span>
              <span>{step.compositeStepCount} nested steps</span>
            </div>
          )}
        </div>
      )}
    </button>
  );
}

function JobNodeComponent({ data }: NodeProps<JobFlowNode>) {
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set());
  const [showAll, setShowAll] = useState(false);

  const toggle = useCallback((idx: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return next;
    });
  }, []);

  const visible = showAll ? data.steps : data.steps.slice(0, MAX_COLLAPSED);
  const extra = data.steps.length - MAX_COLLAPSED;

  return (
    <div
      className={`job-node${data.selected ? " job-node--selected" : ""}${
        data.dimmed ? " job-node--dimmed" : ""
      }`}
      style={{ borderLeftColor: RUN_STATE_COLORS[data.runState] }}
      onClick={() => {
        if (data.dimmed) return;
        data.onSelectJob(data.workflowIndex, data.instanceId, data.file);
      }}
    >
      <div className="job-node__header">
        <div className="job-node__heading">
          <span
            className={`job-node__state job-node__state--${data.runState}`}
            title={RUN_STATE_LABELS[data.runState]}
            aria-hidden="true"
          >
            {data.runState === "willRun"
              ? "✓"
              : data.runState === "skipped"
                ? "⏸"
                : data.runState === "unknown"
                  ? "?"
                  : "–"}
          </span>
          <span className="job-node__title">{data.instanceId}</span>
          <span className="job-node__type">JOB</span>
        </div>
        {data.runsOn && (
          <span className="badge badge-cyan job-node__runs-on" title={data.runsOn}>
            {data.runsOn}
          </span>
        )}
        {data.environmentName && (
          <span
            className="badge badge-magenta job-node__env"
            title={`environment: ${data.environmentName}`}
          >
            env:{data.environmentName}
          </span>
        )}
      </div>

      {(data.needs.length > 0 || data.condition) && (
        <div className="job-node__meta">
          {data.needs.length > 0 && (
            <span className="job-node__meta-item" title={data.needs.join(", ")}>
              depends: {data.needs.join(", ")}
            </span>
          )}
          {data.condition && (
            <span className="job-node__meta-item mono" title={data.condition}>
              if: {truncate(data.condition, 40)}
            </span>
          )}
        </div>
      )}

      {data.support !== "supported" && (
        <div className={`job-node__support job-node__support--${data.support}`}>
          {data.support}
        </div>
      )}

      <div className="job-node__steps">
        {visible.map((step) => {
          const state = data.stepStates.get(step.index) ?? "unknown";
          return (
            <div key={step.index} className="step-card-wrap">
              <Handle
                type="target"
                id={`step-${step.index}-in`}
                position={Position.Left}
                className="step-handle step-handle--in"
              />
              <Handle
                type="source"
                id={`step-${step.index}-out`}
                position={Position.Right}
                className="step-handle step-handle--out"
              />
              <StepCard
                step={step}
                state={state}
                selected={data.selectedStepIdx === step.index}
                expanded={expanded.has(step.index)}
                dimmed={Boolean(data.dimmed)}
                onToggle={() => toggle(step.index)}
                onSelect={() =>
                  data.onSelectStep(
                    data.workflowIndex,
                    data.instanceId,
                    data.file,
                    step.index,
                  )
                }
              />
            </div>
          );
        })}
        {!showAll && extra > 0 && (
          <button
            type="button"
            className="job-node__more"
            onClick={(e) => {
              if (data.dimmed) return;
              e.stopPropagation();
              setShowAll(true);
            }}
          >
            +{extra} more steps
          </button>
        )}
      </div>

      <Handle
        type="target"
        id="job-in"
        position={Position.Left}
        className="job-handle job-handle--in"
      />
      <Handle
        type="source"
        id="job-out"
        position={Position.Right}
        className="job-handle job-handle--out"
      />
    </div>
  );
}

export default memo(JobNodeComponent);
