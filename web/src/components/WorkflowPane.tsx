import { useState } from "react";
import type { WorkflowDto } from "../types";
import { filterWorkflows } from "./WorkflowSwitcher";

function filename(path: string): string {
  return path.replace(/\\/g, "/").split("/").pop() ?? path;
}

export default function WorkflowPane({
  workflows,
  activeIndex,
  onSelect,
  fetchingWorkflowIndex = null,
  onFetchWorkflow,
}: {
  workflows: WorkflowDto[];
  activeIndex: number;
  onSelect: (index: number) => void;
  fetchingWorkflowIndex?: number | null;
  onFetchWorkflow?: (index: number) => void;
}) {
  const [search, setSearch] = useState("");
  const filtered = filterWorkflows(workflows, search);

  return (
    <div className="workflow-pane__list">
      <input
        type="search"
        className="workflow-pane__search"
        placeholder="Search workflows…"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        aria-label="Search workflows"
      />
      <div className="workflow-pane__rows" role="listbox" aria-label="Workflows">
        {filtered.map((w) => {
          const file = filename(w.path);
          const title = w.name ?? file;
          return (
            <div key={w.index} className="workflow-pane__row">
              <button
                type="button"
                role="option"
                aria-selected={w.index === activeIndex}
                className="workflow-pane__option"
                onClick={() => onSelect(w.index)}
              >
                <span className="workflow-pane__name">{title}</span>
                {w.name ? (
                  <span className="workflow-pane__file mono">{file}</span>
                ) : null}
              </button>
              {w.needsRemoteFetch && onFetchWorkflow ? (
                <button
                  type="button"
                  className="btn btn-fetch"
                  disabled={fetchingWorkflowIndex === w.index}
                  onClick={(e) => {
                    e.stopPropagation();
                    onFetchWorkflow(w.index);
                  }}
                >
                  {fetchingWorkflowIndex === w.index ? "Fetching…" : "Fetch"}
                </button>
              ) : null}
            </div>
          );
        })}
      </div>
      {filtered.length === 0 && (
        <p className="workflow-pane__empty">No workflows match</p>
      )}
    </div>
  );
}
