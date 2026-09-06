import { useEffect, useMemo, useRef, useState } from "react";
import type { RunState, Selection, WebView, WorkflowDto } from "../types";
import {
  ALL_RUN_STATES,
  RUN_STATE_COLORS,
  RUN_STATE_LABELS,
  findingsForWorkflow,
  findingsSummary,
  actionableFindings,
  jobState,
  stepState,
} from "../types";

interface TreePaneProps {
  view: WebView;
  activeWorkflowIndex: number;
  selection: Selection | null;
  filterText: string;
  runStateFilters: Set<RunState>;
  foldedWorkflows: Set<number>;
  foldedJobs: Set<string>;
  onToggleWorkflowFold: (index: number) => void;
  onToggleJobFold: (key: string) => void;
  onSelectWorkflow: (index: number) => void;
  onSelectJob: (workflowIndex: number, instanceId: string, file: string) => void;
  onSelectStep: (
    workflowIndex: number,
    instanceId: string,
    file: string,
    stepIdx: number,
  ) => void;
  onToggleRunStateFilter: (state: RunState) => void;
  onFilterChange: (text: string) => void;
  onFetchAll: () => void;
  onFetchWorkflow?: (index: number) => void;
  onOpenScratch: () => void;
  scratchOpen?: boolean;
  scratchDirty?: boolean;
  fetchingAll: boolean;
  fetchingWorkflowIndex?: number | null;
  pendingFetchCount: number;
}

function jobKey(file: string, instanceId: string): string {
  return `${file}::${instanceId}`;
}

function matchesFilter(text: string, filter: string): boolean {
  if (!filter) return true;
  return text.toLowerCase().includes(filter.toLowerCase());
}

function collectFilterSuggestions(view: WebView): string[] {
  const byKey = new Map<string, string>();
  const add = (raw: string | null | undefined) => {
    const text = raw?.trim() ?? "";
    if (text.length < 2) return;
    // Skip full marketplace refs (owner/name@version); keep short action names only.
    if (/^[a-z0-9_.-]+\/[a-z0-9_.-]+@/i.test(text)) return;
    const key = text.toLowerCase();
    const prev = byKey.get(key);
    if (!prev) {
      byKey.set(key, text);
      return;
    }
    // Prefer Title Case / mixed over all-lowercase; prefer shorter labels.
    const prevPlain = prev === prev.toLowerCase();
    const nextMixed = text !== text.toLowerCase();
    if ((prevPlain && nextMixed) || text.length < prev.length) {
      byKey.set(key, text);
    }
  };

  for (const wf of view.workflows) {
    add(wf.name);
    add(wf.path.replace(/\\/g, "/").split("/").pop());
    for (const inst of wf.instances) {
      add(inst.instanceId);
      add(inst.baseId);
      add(inst.environment?.name);
      for (const step of inst.steps) {
        add(step.name);
        add(step.id);
        if (step.uses?.trim()) {
          add(step.uses.split("@")[0]?.split("/").pop());
        }
      }
    }
  }
  return [...byKey.values()];
}

/** Prefix matches first, then shorter labels — so "buil" → build before long substrings. */
function rankFilterSuggestions(items: string[], needle: string): string[] {
  const n = needle.toLowerCase();
  return items
    .filter((item) => {
      const lower = item.toLowerCase();
      return lower.includes(n) && lower !== n;
    })
    .sort((a, b) => {
      const al = a.toLowerCase();
      const bl = b.toLowerCase();
      const aPrefix = al.startsWith(n) ? 0 : 1;
      const bPrefix = bl.startsWith(n) ? 0 : 1;
      if (aPrefix !== bPrefix) return aPrefix - bPrefix;
      const aToken = al.split(/[\s/_-]+/).some((part) => part.startsWith(n))
        ? 0
        : 1;
      const bToken = bl.split(/[\s/_-]+/).some((part) => part.startsWith(n))
        ? 0
        : 1;
      if (aToken !== bToken) return aToken - bToken;
      if (a.length !== b.length) return a.length - b.length;
      return a.localeCompare(b);
    })
    .slice(0, 10);
}

function instanceMatches(
  wf: WorkflowDto,
  instanceId: string,
  view: WebView,
  filter: string,
  runStateFilters: Set<RunState>,
): boolean {
  const inst = wf.instances.find((i) => i.instanceId === instanceId);
  if (!inst) return false;

  const state = jobState(view, wf.path, instanceId);
  if (runStateFilters.size > 0 && !runStateFilters.has(state)) return false;

  if (matchesFilter(instanceId, filter) || matchesFilter(inst.baseId, filter)) {
    return true;
  }

  return inst.steps.some((step) => {
    const sState = stepState(view, wf.path, instanceId, step.index);
    if (runStateFilters.size > 0 && !runStateFilters.has(sState)) return false;
    const name = step.name ?? step.uses ?? step.run ?? "";
    return matchesFilter(name, filter);
  });
}

function workflowMatches(
  wf: WorkflowDto,
  filter: string,
  view: WebView,
  runStateFilters: Set<RunState>,
): boolean {
  const label = wf.name ?? wf.path;
  if (matchesFilter(label, filter) || matchesFilter(wf.path, filter)) {
    return true;
  }
  return wf.instances.some((inst) =>
    instanceMatches(wf, inst.instanceId, view, filter, runStateFilters),
  );
}

function isSelected(
  selection: Selection | null,
  kind: Selection["kind"],
  workflowIndex: number,
  instanceId?: string,
  stepIdx?: number,
): boolean {
  if (!selection || selection.kind !== kind) return false;
  if (selection.workflowIndex !== workflowIndex) return false;
  if (kind === "workflow") return true;
  if (kind === "job") {
    return selection.kind === "job" && selection.instanceId === instanceId;
  }
  return (
    selection.kind === "step" &&
    selection.instanceId === instanceId &&
    selection.stepIdx === stepIdx
  );
}

export default function TreePane({
  view,
  activeWorkflowIndex,
  selection,
  filterText,
  runStateFilters,
  foldedWorkflows,
  foldedJobs,
  onToggleWorkflowFold,
  onToggleJobFold,
  onSelectWorkflow,
  onSelectJob,
  onSelectStep,
  onToggleRunStateFilter,
  onFilterChange,
  onFetchAll,
  onFetchWorkflow,
  onOpenScratch,
  scratchOpen = false,
  scratchDirty = false,
  fetchingAll,
  fetchingWorkflowIndex = null,
  pendingFetchCount,
}: TreePaneProps) {
  const totalFindings = actionableFindings(view).length;
  const [suggestOpen, setSuggestOpen] = useState(false);
  const [activeSuggest, setActiveSuggest] = useState(0);
  const filterWrapRef = useRef<HTMLDivElement>(null);

  const allSuggestions = useMemo(
    () => collectFilterSuggestions(view),
    [view],
  );

  const suggestions = useMemo(() => {
    const needle = filterText.trim();
    if (!needle) return [];
    return rankFilterSuggestions(allSuggestions, needle);
  }, [allSuggestions, filterText]);

  useEffect(() => {
    setActiveSuggest(0);
  }, [filterText]);

  useEffect(() => {
    const onDoc = (event: MouseEvent) => {
      if (!filterWrapRef.current?.contains(event.target as Node)) {
        setSuggestOpen(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, []);

  const applySuggestion = (value: string) => {
    onFilterChange(value);
    setSuggestOpen(false);
  };

  const visibleWorkflows = useMemo(
    () =>
      view.workflows.filter((wf) =>
        workflowMatches(wf, filterText, view, runStateFilters),
      ),
    [view, filterText, runStateFilters],
  );

  return (
    <div className="tree-pane">
      <div className="tree-pane__toolbar">
        <div className="tree-filter" ref={filterWrapRef}>
          <input
            className="input"
            type="search"
            placeholder="Filter workflows, jobs, steps…"
            value={filterText}
            onChange={(e) => {
              onFilterChange(e.target.value);
              setSuggestOpen(true);
            }}
            onFocus={() => setSuggestOpen(true)}
            onKeyDown={(e) => {
              if (!suggestOpen || suggestions.length === 0) return;
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActiveSuggest((i) => (i + 1) % suggestions.length);
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActiveSuggest(
                  (i) => (i - 1 + suggestions.length) % suggestions.length,
                );
              } else if (e.key === "Enter" && suggestions[activeSuggest]) {
                e.preventDefault();
                applySuggestion(suggestions[activeSuggest]);
              } else if (e.key === "Escape") {
                setSuggestOpen(false);
              }
            }}
            aria-autocomplete="list"
            aria-expanded={suggestOpen && suggestions.length > 0}
            aria-controls="tree-filter-suggestions"
          />
          {suggestOpen && suggestions.length > 0 && (
            <ul
              id="tree-filter-suggestions"
              className="tree-filter__suggestions"
              role="listbox"
            >
              {suggestions.map((item, index) => (
                <li key={item} role="option" aria-selected={index === activeSuggest}>
                  <button
                    type="button"
                    className={`tree-filter__suggestion${
                      index === activeSuggest
                        ? " tree-filter__suggestion--active"
                        : ""
                    }`}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => applySuggestion(item)}
                  >
                    {item}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="tree-pane__filters">
          {ALL_RUN_STATES.map((state) => (
            <button
              key={state}
              type="button"
              className={`filter-chip${
                runStateFilters.has(state) ? " filter-chip--active" : ""
              }`}
              style={
                runStateFilters.has(state)
                  ? {
                      borderColor: RUN_STATE_COLORS[state],
                      color: RUN_STATE_COLORS[state],
                    }
                  : undefined
              }
              onClick={() => onToggleRunStateFilter(state)}
              title={RUN_STATE_LABELS[state]}
            >
              {RUN_STATE_LABELS[state]}
            </button>
          ))}
        </div>
        <div className="tree-pane__meta">
          <span
            className="findings-count"
            title={findingsSummary(view)}
          >
            {totalFindings} issue{totalFindings === 1 ? "" : "s"}
          </span>
          {pendingFetchCount > 0 && (
            <button
              type="button"
              className="btn btn-fetch"
              disabled={fetchingAll}
              onClick={onFetchAll}
              title="Download missing remote actions into ~/.cache/gha-see (explicit network)"
            >
              {fetchingAll
                ? "Fetching…"
                : `Fetch remotes (${pendingFetchCount})`}
            </button>
          )}
        </div>
        <div className="tree-pane__actions" aria-label="Workflow actions">
          <button
            type="button"
            className={`btn btn-primary tree-pane__action${scratchOpen ? " tree-pane__action--active" : ""}`}
            onClick={onOpenScratch}
            aria-pressed={scratchOpen}
            title={
              scratchOpen
                ? "Hide workflow YAML editor"
                : "View & edit workflow YAML"
            }
          >
            View & edit YAML{scratchDirty ? " · unsaved" : ""}
          </button>
        </div>
      </div>

      <div className="tree-pane__list">
        {visibleWorkflows.length === 0 ? (
          <div className="tree-pane__empty">No workflows match filter</div>
        ) : (
          visibleWorkflows.map((wf) => {
            const wfFindings = findingsForWorkflow(view, wf.path);
            const active = wf.index === activeWorkflowIndex;
            const wfSelected = isSelected(selection, "workflow", wf.index);

            const wfLabel = wf.name ?? wf.path;
            const wfNameMatch =
              !filterText ||
              matchesFilter(wfLabel, filterText) ||
              matchesFilter(wf.path, filterText);

            const visibleInstances = wf.instances.filter((inst) => {
              if (wfNameMatch && filterText) {
                const state = jobState(view, wf.path, inst.instanceId);
                if (runStateFilters.size > 0 && !runStateFilters.has(state)) {
                  return false;
                }
                return true;
              }
              return instanceMatches(
                wf,
                inst.instanceId,
                view,
                filterText,
                runStateFilters,
              );
            });

            // While filtering, force-expand matches so jobs/steps stay visible.
            const folded =
              Boolean(filterText.trim()) &&
              (wfNameMatch || visibleInstances.length > 0)
                ? false
                : foldedWorkflows.has(wf.index);

            return (
              <div
                key={wf.index}
                className={`tree-wf${active ? " tree-wf--active" : ""}${
                  wfSelected ? " tree-wf--selected" : ""
                }`}
              >
                <div className="tree-row tree-row--wf">
                  <button
                    type="button"
                    className={`tree-fold${folded ? "" : " tree-fold--open"}`}
                    onClick={() => onToggleWorkflowFold(wf.index)}
                    aria-label={folded ? "Expand workflow" : "Collapse workflow"}
                    aria-expanded={!folded}
                  >
                    <span className="tree-fold__chevron" aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    className="tree-label"
                    onClick={() => onSelectWorkflow(wf.index)}
                  >
                    <span className="tree-name">
                      {wf.name ?? wf.path.split("/").pop()}
                    </span>
                    {!wf.parseOk && (
                      <span className="tree-badge tree-badge--error">parse</span>
                    )}
                    {wfFindings > 0 && (
                      <span
                        className="tree-badge tree-badge--findings"
                        title={findingsSummary(view, wf.path)}
                      >
                        {wfFindings}
                      </span>
                    )}
                    {wf.needsRemoteFetch && (
                      <span
                        className="tree-badge tree-badge--fetch"
                        title="Remote uses not in local cache yet"
                      >
                        remote
                      </span>
                    )}
                  </button>
                  {wf.needsRemoteFetch && onFetchWorkflow && (
                    <button
                      type="button"
                      className="btn btn-fetch tree-row__fetch"
                      disabled={
                        fetchingAll || fetchingWorkflowIndex === wf.index
                      }
                      onClick={(e) => {
                        e.stopPropagation();
                        onFetchWorkflow(wf.index);
                      }}
                      title="Download missing remote actions for this workflow"
                    >
                      {fetchingWorkflowIndex === wf.index
                        ? "Fetching…"
                        : "Fetch"}
                    </button>
                  )}
                </div>

                {!folded &&
                  visibleInstances.map((inst) => {
                    const jKey = jobKey(wf.path, inst.instanceId);
                    const jobFolded = foldedJobs.has(jKey);
                    const state = jobState(view, wf.path, inst.instanceId);
                    const jobSel = isSelected(
                      selection,
                      "job",
                      wf.index,
                      inst.instanceId,
                    );

                    const visibleSteps = inst.steps.filter((step) => {
                      const sState = stepState(
                        view,
                        wf.path,
                        inst.instanceId,
                        step.index,
                      );
                      if (
                        runStateFilters.size > 0 &&
                        !runStateFilters.has(sState) &&
                        !runStateFilters.has(state)
                      ) {
                        return false;
                      }
                      const name = step.name ?? step.uses ?? step.run ?? "";
                      return (
                        !filterText ||
                        matchesFilter(name, filterText) ||
                        matchesFilter(inst.instanceId, filterText)
                      );
                    });

                    return (
                      <div key={jKey} className="tree-job-group">
                        <div
                          className={`tree-row tree-row--job${
                            jobSel ? " tree-row--selected" : ""
                          }`}
                        >
                          <button
                            type="button"
                            className={`tree-fold${jobFolded ? "" : " tree-fold--open"}`}
                            onClick={() => onToggleJobFold(jKey)}
                            aria-label={jobFolded ? "Expand job" : "Collapse job"}
                            aria-expanded={!jobFolded}
                          >
                            <span className="tree-fold__chevron" aria-hidden="true" />
                          </button>
                          <span
                            className="tree-state-dot"
                            style={{ background: RUN_STATE_COLORS[state] }}
                          />
                          <button
                            type="button"
                            className="tree-label"
                            onClick={() =>
                              onSelectJob(wf.index, inst.instanceId, wf.path)
                            }
                          >
                            {inst.instanceId}
                          </button>
                        </div>

                        {!jobFolded &&
                          visibleSteps.map((step) => {
                            const sState = stepState(
                              view,
                              wf.path,
                              inst.instanceId,
                              step.index,
                            );
                            const stepSel = isSelected(
                              selection,
                              "step",
                              wf.index,
                              inst.instanceId,
                              step.index,
                            );
                            const label =
                              step.name ??
                              step.uses ??
                              step.run ??
                              `step ${step.index}`;

                            return (
                              <button
                                key={step.index}
                                type="button"
                                className={`tree-row tree-row--step${
                                  stepSel ? " tree-row--selected" : ""
                                }`}
                                onClick={() =>
                                  onSelectStep(
                                    wf.index,
                                    inst.instanceId,
                                    wf.path,
                                    step.index,
                                  )
                                }
                              >
                                <span
                                  className="tree-state-dot tree-state-dot--small"
                                  style={{
                                    background: RUN_STATE_COLORS[sState],
                                  }}
                                />
                                <span className="tree-step-label">{label}</span>
                              </button>
                            );
                          })}
                      </div>
                    );
                  })}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
