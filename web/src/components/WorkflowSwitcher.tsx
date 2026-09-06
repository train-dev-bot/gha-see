import { useEffect, useRef, useState } from "react";
import type { WorkflowDto } from "../types";

function filename(path: string): string {
  return path.replace(/\\/g, "/").split("/").pop() ?? path;
}

export function filterWorkflows(
  workflows: WorkflowDto[],
  query: string,
): WorkflowDto[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...workflows];
  return workflows.filter((w) => {
    const name = (w.name ?? "").toLowerCase();
    const path = w.path.toLowerCase();
    return name.includes(needle) || path.includes(needle);
  });
}

export default function WorkflowSwitcher({
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
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  const active =
    workflows.find((w) => w.index === activeIndex) ?? workflows[0] ?? null;
  const filtered = filterWorkflows(workflows, search);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const label = active ? (active.name ?? filename(active.path)) : "Workflow";
  const close = () => {
    setOpen(false);
    setSearch("");
  };

  return (
    <div className="workflow-switcher" ref={rootRef}>
      <button
        type="button"
        className="btn workflow-switcher__button mono"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {label}
      </button>
      {open && (
        <div className="workflow-switcher__menu" role="presentation">
          <input
            type="search"
            className="workflow-switcher__search"
            placeholder="Search workflows…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            autoFocus
          />
          <div className="workflow-switcher__body">
            <div
              role="listbox"
              className="workflow-switcher__options"
              aria-label="Workflows"
            >
              {filtered.map((w) => (
                <button
                  key={w.index}
                  type="button"
                  role="option"
                  aria-selected={w.index === activeIndex}
                  className="workflow-switcher__option"
                  onClick={() => {
                    onSelect(w.index);
                    close();
                  }}
                >
                  {w.name ?? filename(w.path)}
                </button>
              ))}
            </div>
            <div className="workflow-switcher__side">
              {filtered.map((w) =>
                w.needsRemoteFetch && onFetchWorkflow ? (
                  <button
                    key={w.index}
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
                ) : (
                  <span
                    key={w.index}
                    className="workflow-switcher__side-spacer"
                  />
                ),
              )}
            </div>
          </div>
          {filtered.length === 0 && (
            <p className="workflow-switcher__empty">No workflows match</p>
          )}
        </div>
      )}
    </div>
  );
}
