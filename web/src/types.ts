export type RunState = "willRun" | "skipped" | "unknown" | "deferred";

export interface WebView {
  path: string;
  workflows: WorkflowDto[];
  graphs: GraphDto[];
  bindings: BindingDto[];
  findings: FindingDto[];
  jobStates: JobStateDto[];
  stepStates: StepStateDto[];
  context: EvalContextDto;
}

export interface WorkflowDto {
  index: number;
  path: string;
  name: string | null;
  parseOk: boolean;
  fileError: string | null;
  rawSource: string | null;
  hasRemoteUses: boolean;
  needsRemoteFetch: boolean;
  instances: InstanceDto[];
  dispatchInputs: DispatchInputDto[];
}

export interface DispatchInputDto {
  name: string;
  default: string | null;
  required: boolean;
}

export interface EnvironmentDto {
  name: string;
  url: string | null;
}

export interface ConcurrencyDto {
  group: string;
  cancelInProgress: boolean | null;
}

export interface ServiceDto {
  name: string;
  image: string | null;
}

export interface InstanceDto {
  instanceId: string;
  baseId: string;
  matrix: Record<string, string>;
  runsOn: string | null;
  needs: string[];
  condition: string | null;
  outputs: Record<string, string>;
  steps: StepDto[];
  support: "supported" | "deferred" | "unknown";
  environment: EnvironmentDto | null;
  concurrency: ConcurrencyDto | null;
  services: ServiceDto[];
}

export interface StepDto {
  index: number;
  name: string | null;
  id: string | null;
  uses: string | null;
  run: string | null;
  shell: string | null;
  workingDirectory: string | null;
  condition: string | null;
  withInputs: Record<string, string>;
  env: Record<string, string>;
  resolvedActionName: string | null;
  actionRunner: string | null;
  compositeStepCount: number;
}

export interface GraphDto {
  workflowIndex: number;
  nodes: string[];
  edges: [string, string][];
}

export interface BindingDto {
  file: string;
  consumerJob: string;
  /** Step that references the output; null if the ref is on a job-level field. */
  consumerStepIdx: number | null;
  producerJob: string;
  outputName: string;
  raw: string;
}

export interface FindingDto {
  severity: "error" | "warning" | "info";
  code: string;
  message: string;
  coach: string;
  target:
    | { kind: "file"; file: string }
    | { kind: "job"; file: string; job: string }
    | { kind: "step"; file: string; job: string; stepIdx: number };
}

export interface JobStateDto {
  file: string;
  instanceId: string;
  state: RunState;
}

export interface StepStateDto {
  file: string;
  instanceId: string;
  stepIdx: number;
  state: RunState;
}

export interface EvalContextDto {
  eventName: string;
  refName: string;
  github: { sha: string; repository: string; actor: string };
  env: Record<string, string>;
  vars: Record<string, string>;
  inputs: Record<string, string>;
  secrets: string[];
  needs: Record<string, { result: string; outputs: Record<string, string> }>;
  repoRoot: string | null;
}

export type Selection =
  | { kind: "workflow"; workflowIndex: number }
  | { kind: "job"; workflowIndex: number; instanceId: string; file: string }
  | {
      kind: "step";
      workflowIndex: number;
      instanceId: string;
      file: string;
      stepIdx: number;
    };

export type StepKind = "uses" | "run" | "action" | "empty";

export interface JobNodeData extends Record<string, unknown> {
  instanceId: string;
  file: string;
  workflowIndex: number;
  runsOn: string | null;
  runState: RunState;
  steps: StepDto[];
  stepStates: Map<number, RunState>;
  selected: boolean;
  selectedStepIdx: number | null;
  /** True when an edge is focused and this node is not an endpoint. */
  dimmed?: boolean;
  support: InstanceDto["support"];
  condition: string | null;
  needs: string[];
  environmentName: string | null;
  onSelectJob: (workflowIndex: number, instanceId: string, file: string) => void;
  onSelectStep: (
    workflowIndex: number,
    instanceId: string,
    file: string,
    stepIdx: number,
  ) => void;
}

export const RUN_STATE_COLORS: Record<RunState, string> = {
  willRun: "var(--will-run)",
  skipped: "var(--skipped)",
  unknown: "var(--unknown)",
  deferred: "var(--deferred)",
};

export const RUN_STATE_LABELS: Record<RunState, string> = {
  willRun: "Will run",
  skipped: "Skipped",
  unknown: "Unknown",
  deferred: "Deferred",
};

export const ALL_RUN_STATES: RunState[] = [
  "willRun",
  "skipped",
  "unknown",
  "deferred",
];

export function stepKind(step: StepDto): StepKind {
  if (step.uses) return step.actionRunner ? "action" : "uses";
  if (step.run) return "run";
  return "empty";
}

export function stepTitle(step: StepDto): string {
  if (step.name) return step.name;
  if (step.uses) return step.uses;
  if (step.run) {
    const first = step.run.trim().split("\n")[0] ?? "run";
    return first.length > 48 ? `${first.slice(0, 48)}…` : first;
  }
  return `step ${step.index}`;
}

export function stepSubtitle(step: StepDto): string | null {
  if (step.name && step.uses) return step.uses;
  if (step.name && step.run) {
    const first = step.run.trim().split("\n")[0] ?? "";
    return first.length > 56 ? `${first.slice(0, 56)}…` : first || null;
  }
  if (step.resolvedActionName) return step.resolvedActionName;
  if (step.actionRunner) return step.actionRunner;
  return null;
}

export function jobState(
  view: WebView,
  file: string,
  instanceId: string,
): RunState {
  return (
    view.jobStates.find((s) => s.file === file && s.instanceId === instanceId)
      ?.state ?? "unknown"
  );
}

export function stepState(
  view: WebView,
  file: string,
  instanceId: string,
  stepIdx: number,
): RunState {
  return (
    view.stepStates.find(
      (s) =>
        s.file === file &&
        s.instanceId === instanceId &&
        s.stepIdx === stepIdx,
    )?.state ?? "unknown"
  );
}

export function findingsForWorkflow(
  view: WebView,
  workflowPath: string,
): number {
  return actionableFindings(view).filter((f) => f.target.file === workflowPath)
    .length;
}

/** Issues that deserve attention — excludes info-level condition skips. */
export function isActionableFinding(f: FindingDto): boolean {
  if (f.severity === "info") return false;
  if (f.code === "GHA_COND_SKIP") return false;
  return true;
}

export function actionableFindings(view: WebView): FindingDto[] {
  return view.findings.filter(isActionableFinding);
}

export function findingsSummary(view: WebView, workflowPath?: string): string {
  const list = actionableFindings(view).filter((f) =>
    workflowPath ? f.target.file === workflowPath : true,
  );
  if (list.length === 0) return "No warnings or errors";
  const errors = list.filter((f) => f.severity === "error").length;
  const warnings = list.filter((f) => f.severity === "warning").length;
  const parts: string[] = [];
  if (errors) parts.push(`${errors} error${errors === 1 ? "" : "s"}`);
  if (warnings) parts.push(`${warnings} warning${warnings === 1 ? "" : "s"}`);
  const preview = list
    .slice(0, 3)
    .map((f) => f.message)
    .join(" · ");
  return `${parts.join(", ")}: ${preview}${list.length > 3 ? "…" : ""}`;
}

export function skippedJobsCount(view: WebView, workflowPath?: string): number {
  return view.jobStates.filter((s) => {
    if (s.state !== "skipped") return false;
    if (workflowPath && s.file !== workflowPath) return false;
    return true;
  }).length;
}

export function severityClass(severity: string): string {
  switch (severity) {
    case "error":
      return "finding-error";
    case "warning":
      return "finding-warning";
    default:
      return "finding-info";
  }
}

/** Map a binding job id (base or instance) onto a graph instance id. */
export function resolveInstanceId(
  instances: InstanceDto[],
  jobId: string,
): string | null {
  const exact = instances.find((i) => i.instanceId === jobId);
  if (exact) return exact.instanceId;
  const byBase = instances.filter((i) => i.baseId === jobId);
  if (byBase.length === 1) return byBase[0].instanceId;
  if (byBase.length > 1) return byBase[0].instanceId;
  return null;
}
