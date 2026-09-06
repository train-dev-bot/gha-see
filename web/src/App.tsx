import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import {
  analyzePath,
  analyzeSource,
  fetchAllRemotes,
  fetchRemotes,
  fetchView,
  revaluate,
  saveWorkflow,
} from "./api";
import AppHeader from "./components/AppHeader";
import ContextSheet from "./components/ContextSheet";
import GraphPane from "./components/GraphPane";
import InspectorPane from "./components/InspectorPane";
import PathBrowser, {
  openFolderPath,
  workflowIndexForPath,
} from "./components/PathBrowser";
import WorkflowPane from "./components/WorkflowPane";
import YamlEditor, { EMPTY_TEMPLATE } from "./components/YamlEditor";
import { preservedActiveIndex } from "./preserveWorkflow";
import type { EvalContextDto, RunState, Selection, WebView } from "./types";
import {
  actionableFindings,
  skippedJobsCount,
} from "./types";
import "./App.css";

function selectionStillValid(data: WebView, sel: Selection | null): Selection | null {
  if (!sel) return null;
  const wf = data.workflows.find((w) => w.index === sel.workflowIndex);
  if (!wf) return null;
  if (sel.kind === "workflow") return sel;
  const inst = wf.instances.find((i) => i.instanceId === sel.instanceId);
  if (!inst) return { kind: "workflow", workflowIndex: sel.workflowIndex };
  if (sel.kind === "job") return sel;
  if (sel.stepIdx < 0 || sel.stepIdx >= inst.steps.length) {
    return {
      kind: "job",
      workflowIndex: sel.workflowIndex,
      instanceId: sel.instanceId,
      file: sel.file,
    };
  }
  return sel;
}

export default function App() {
  const [view, setView] = useState<WebView | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [fetchingAll, setFetchingAll] = useState(false);
  const [fetchingWorkflowIndex, setFetchingWorkflowIndex] = useState<number | null>(
    null,
  );
  const [reloading, setReloading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showBrowser, setShowBrowser] = useState(false);
  const [showEditor, setShowEditor] = useState(false);
  const [editorDirty, setEditorDirty] = useState(false);
  const [editorDiscardPrompt, setEditorDiscardPrompt] = useState(false);
  const [editorSource, setEditorSource] = useState(EMPTY_TEMPLATE);
  const [editorName, setEditorName] = useState("scratch.yml");
  const [editorSavePath, setEditorSavePath] = useState(
    "./.github/workflows/scratch.yml",
  );
  const [drawerHeight, setDrawerHeight] = useState(() =>
    Math.round(Math.min(window.innerHeight * 0.5, window.innerHeight - 120)),
  );

  const [activeWorkflowIndex, setActiveWorkflowIndex] = useState(0);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [runStateFilters, setRunStateFilters] = useState<Set<RunState>>(
    () => new Set(),
  );
  const [sheetOpen, setSheetOpen] = useState(true);
  const [sheetTab, setSheetTab] = useState<"context" | "selection">("context");
  const [sheetWidth, setSheetWidth] = useState(() =>
    Math.round(Math.min(window.innerWidth * 0.32, 420)),
  );
  const [workflowsOpen, setWorkflowsOpen] = useState(true);
  const [workflowsWidth, setWorkflowsWidth] = useState(() =>
    Math.round(Math.min(window.innerWidth * 0.22, 280)),
  );
  /** Last real on-disk folder/file — Browse/Reload use this, not scratch paths. */
  const [diskPath, setDiskPath] = useState(".");

  const selectionRef = useRef(selection);
  const activeWorkflowIndexRef = useRef(activeWorkflowIndex);
  const viewRef = useRef(view);
  useEffect(() => {
    selectionRef.current = selection;
  }, [selection]);
  useEffect(() => {
    activeWorkflowIndexRef.current = activeWorkflowIndex;
  }, [activeWorkflowIndex]);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  const isScratchPath = useCallback((path: string) => path.startsWith("(scratch)"), []);

  const rememberDiskPath = useCallback(
    (path: string) => {
      if (!isScratchPath(path)) {
        setDiskPath(path);
      }
    },
    [isScratchPath],
  );

  const applyView = useCallback(
    (
      data: WebView,
      opts?: {
        rememberDisk?: boolean;
        preserveSelection?: boolean;
        selectPath?: string | null;
      },
    ) => {
      setView(data);
      if (opts?.rememberDisk !== false) {
        rememberDiskPath(data.path);
      }

      const firstIndex = data.workflows[0]?.index;
      let active: number;
      let nextSelection: Selection | null;

      if (firstIndex === undefined) {
        active = 0;
        nextSelection = null;
      } else if (opts?.selectPath) {
        active =
          workflowIndexForPath(data.workflows, opts.selectPath) ?? firstIndex;
        nextSelection = { kind: "workflow", workflowIndex: active };
      } else if (opts?.preserveSelection) {
        const prevWf = viewRef.current?.workflows.find(
          (w) => w.index === activeWorkflowIndexRef.current,
        );
        active = preservedActiveIndex(
          data.workflows,
          prevWf
            ? { index: prevWf.index, path: prevWf.path }
            : { index: activeWorkflowIndexRef.current, path: "" },
        );
        const prevSel = selectionRef.current;
        const prevSelWf =
          prevSel && viewRef.current
            ? viewRef.current.workflows.find(
                (w) => w.index === prevSel.workflowIndex,
              )
            : undefined;
        const remappedSel =
          prevSel && prevSelWf
            ? {
                ...prevSel,
                workflowIndex:
                  data.workflows.find((w) => w.path === prevSelWf.path)
                    ?.index ?? active,
              }
            : prevSel;
        nextSelection =
          selectionStillValid(data, remappedSel ?? null) ?? {
            kind: "workflow",
            workflowIndex: active,
          };
      } else {
        active = firstIndex;
        nextSelection = { kind: "workflow", workflowIndex: firstIndex };
      }

      setActiveWorkflowIndex(active);
      setSelection(nextSelection);
    },
    [rememberDiskPath],
  );

  const pendingFetchCount = useMemo(
    () => view?.workflows.filter((w) => w.needsRemoteFetch).length ?? 0,
    [view],
  );

  const loadView = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchView();
      applyView(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [applyView]);

  useEffect(() => {
    void loadView();
  }, [loadView]);

  const handleSelectWorkflow = useCallback((workflowIndex: number) => {
    setActiveWorkflowIndex(workflowIndex);
    setSelection({ kind: "workflow", workflowIndex });
  }, []);

  const handleSelectJob = useCallback(
    (workflowIndex: number, instanceId: string, file: string) => {
      setActiveWorkflowIndex(workflowIndex);
      setSelection({ kind: "job", workflowIndex, instanceId, file });
    },
    [],
  );

  const handleSelectStep = useCallback(
    (
      workflowIndex: number,
      instanceId: string,
      file: string,
      stepIdx: number,
    ) => {
      setActiveWorkflowIndex(workflowIndex);
      setSelection({ kind: "step", workflowIndex, instanceId, file, stepIdx });
    },
    [],
  );

  const handleToggleRunStateFilter = useCallback((state: RunState) => {
    setRunStateFilters((prev) => {
      const next = new Set(prev);
      if (next.has(state)) next.delete(state);
      else next.add(state);
      return next;
    });
  }, []);

  const onDrawerResizeStart = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startY = e.clientY;
    const startHeight = drawerHeight;
    const target = e.currentTarget;
    target.setPointerCapture(e.pointerId);

    const onMove = (ev: PointerEvent) => {
      const delta = startY - ev.clientY;
      const next = Math.round(startHeight + delta);
      const min = 220;
      const max = Math.max(min, window.innerHeight - 80);
      setDrawerHeight(Math.min(max, Math.max(min, next)));
    };
    const onUp = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", onMove);
      target.removeEventListener("pointerup", onUp);
      target.removeEventListener("pointercancel", onUp);
    };
    target.addEventListener("pointermove", onMove);
    target.addEventListener("pointerup", onUp);
    target.addEventListener("pointercancel", onUp);
  }, [drawerHeight]);

  const onSheetResizeStart = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = sheetWidth;
    const target = e.currentTarget;
    target.setPointerCapture(e.pointerId);

    const onMove = (ev: PointerEvent) => {
      const delta = startX - ev.clientX;
      const next = Math.round(startWidth + delta);
      const min = 280;
      const max = Math.max(min, Math.round(window.innerWidth * 0.6));
      setSheetWidth(Math.min(max, Math.max(min, next)));
    };
    const onUp = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", onMove);
      target.removeEventListener("pointerup", onUp);
      target.removeEventListener("pointercancel", onUp);
    };
    target.addEventListener("pointermove", onMove);
    target.addEventListener("pointerup", onUp);
    target.addEventListener("pointercancel", onUp);
  }, [sheetWidth]);

  const onWorkflowsResizeStart = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = workflowsWidth;
    const target = e.currentTarget;
    target.setPointerCapture(e.pointerId);

    const onMove = (ev: PointerEvent) => {
      const delta = ev.clientX - startX;
      const next = Math.round(startWidth + delta);
      const min = 200;
      const max = Math.max(min, Math.round(window.innerWidth * 0.45));
      setWorkflowsWidth(Math.min(max, Math.max(min, next)));
    };
    const onUp = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", onMove);
      target.removeEventListener("pointerup", onUp);
      target.removeEventListener("pointercancel", onUp);
    };
    target.addEventListener("pointermove", onMove);
    target.addEventListener("pointerup", onUp);
    target.addEventListener("pointercancel", onUp);
  }, [workflowsWidth]);

  const handleApplyContext = useCallback(async (context: EvalContextDto) => {
    setBusy(true);
    setError(null);
    try {
      const updated = await revaluate(context);
      applyView(updated, {
        preserveSelection: true,
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [applyView]);

  const handleFetchAll = useCallback(async () => {
    setBusy(true);
    setFetchingAll(true);
    setError(null);
    try {
      const updated = await fetchAllRemotes();
      applyView(updated, {
        preserveSelection: true,
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setFetchingAll(false);
    }
  }, [applyView]);

  const handleFetchWorkflow = useCallback(
    async (workflowIndex: number) => {
      setBusy(true);
      setFetchingWorkflowIndex(workflowIndex);
      setError(null);
      try {
        const updated = await fetchRemotes(workflowIndex);
        applyView(updated, {
          preserveSelection: true,
        });
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
        setFetchingWorkflowIndex(null);
      }
    },
    [applyView],
  );

  const handleReload = useCallback(async () => {
    setBusy(true);
    setReloading(true);
    setError(null);
    try {
      applyView(await analyzePath(diskPath), { preserveSelection: true });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setReloading(false);
    }
  }, [applyView, diskPath]);

  const handleBrowseOpen = useCallback(
    async (path: string) => {
      setBusy(true);
      setError(null);
      try {
        const { folder, selectFile } = openFolderPath(path);
        applyView(await analyzePath(folder), { selectPath: selectFile });
        setShowBrowser(false);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [applyView],
  );

  const openScratchEditor = useCallback(() => {
    const workflow = view?.workflows.find((w) => w.index === activeWorkflowIndex);
    const rawPath = workflow?.path ?? "";
    const fileName =
      rawPath
        .replace(/\\/g, "/")
        .split("/")
        .pop()
        ?.replace(/^\(scratch\)/, "") || "scratch.yml";
    const cleanName = fileName || "scratch.yml";
    setEditorSource(workflow?.rawSource || EMPTY_TEMPLATE);
    setEditorName(cleanName);
    const baseDir =
      diskPath.endsWith(".yml") || diskPath.endsWith(".yaml")
        ? diskPath.replace(/[/\\][^/\\]+$/, "")
        : diskPath;
    const joined = `${baseDir.replace(/[/\\]$/, "")}/${cleanName}`;
    const saveTarget =
      rawPath && !isScratchPath(rawPath) ? rawPath : joined;
    setEditorSavePath(saveTarget);
    setEditorDirty(false);
    setEditorDiscardPrompt(false);
    setShowEditor(true);
  }, [activeWorkflowIndex, diskPath, isScratchPath, view]);

  const closeScratchEditor = useCallback((force = false) => {
    if (!force && editorDirty) {
      setEditorDiscardPrompt(true);
      return;
    }
    setEditorDiscardPrompt(false);
    setEditorDirty(false);
    setShowEditor(false);
  }, [editorDirty]);

  const toggleScratchEditor = useCallback(() => {
    if (showEditor) {
      closeScratchEditor(false);
      return;
    }
    openScratchEditor();
  }, [showEditor, closeScratchEditor, openScratchEditor]);

  const handleAnalyzeSource = useCallback(
    async (name: string, source: string) => {
      setBusy(true);
      setError(null);
      try {
        // Do not treat scratch as the new disk root.
        applyView(await analyzeSource(name, source), { rememberDisk: false });
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        throw e;
      } finally {
        setBusy(false);
      }
    },
    [applyView],
  );

  const handleSaveWorkflow = useCallback(
    async (path: string, source: string) => {
      setBusy(true);
      setError(null);
      try {
        applyView(await saveWorkflow(path, source));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        throw e;
      } finally {
        setBusy(false);
      }
    },
    [applyView],
  );

  if (loading && !view) {
    return (
      <div className="empty-state">
        <p>Loading workflow analysis…</p>
      </div>
    );
  }

  if (!view || view.workflows.length === 0) {
    return (
      <>
        <div className="empty-state">
          <h1>gha-see</h1>
          <p>No workflows found</p>
          {error && <div className="error-banner">{error}</div>}
          <div className="empty-state__actions">
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void loadView()}
            >
              Retry
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => setShowBrowser(true)}
            >
              Browse
            </button>
            <button
              type="button"
              className="btn"
              onClick={toggleScratchEditor}
              aria-pressed={showEditor}
            >
              View & edit YAML
            </button>
          </div>
        </div>
        {showBrowser && (
          <PathBrowser
            initialPath={diskPath}
            onOpen={(path) => void handleBrowseOpen(path)}
            onClose={() => setShowBrowser(false)}
          />
        )}
        {editorDiscardPrompt && (
          <div className="scratch-discard" role="alertdialog" aria-labelledby="scratch-discard-title">
            <p id="scratch-discard-title">
              Workflow YAML has unsaved changes. Close and discard them?
            </p>
            <div className="scratch-discard__actions">
              <button
                type="button"
                className="btn"
                onClick={() => setEditorDiscardPrompt(false)}
              >
                Keep editing
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => closeScratchEditor(true)}
              >
                Discard and close
              </button>
            </div>
          </div>
        )}
        {showEditor && (
          <div className="app-drawer" style={{ height: drawerHeight }}>
            <div
              className="app-drawer__resize"
              onPointerDown={onDrawerResizeStart}
              title="Drag to resize"
              role="separator"
              aria-orientation="horizontal"
              aria-label="Resize YAML editor"
            />
            <YamlEditor
              initialSource={editorSource}
              suggestedName={editorName}
              suggestedSavePath={editorSavePath}
              busy={busy}
              onAnalyze={handleAnalyzeSource}
              onSave={handleSaveWorkflow}
              onClose={() => closeScratchEditor(false)}
              onDirtyChange={setEditorDirty}
            />
          </div>
        )}
      </>
    );
  }

  const issueCount = actionableFindings(view).length;

  return (
    <div className="app-shell">
      <AppHeader
        pathLabel={view.path.startsWith("(scratch)") ? diskPath : view.path}
        showScratchBadge={view.workflows.some((w) => w.path.startsWith("(scratch)"))}
        reloading={reloading}
        issueCount={issueCount}
        skippedCount={skippedJobsCount(view)}
        pendingFetchCount={pendingFetchCount}
        fetchingAll={fetchingAll}
        showEditor={showEditor}
        editorDirty={editorDirty}
        onReload={() => void handleReload()}
        onBrowse={() => setShowBrowser(true)}
        onFetchAll={() => void handleFetchAll()}
        onToggleEditor={toggleScratchEditor}
      />

      <main className="app-stage">
        {error && <div className="error-banner">{error}</div>}
        {busy && <div className="loading-indicator" role="status">Updating analysis…</div>}
        <section
          className={`workflow-pane${workflowsOpen ? "" : " workflow-pane--collapsed"}`}
          style={{ width: workflowsOpen ? workflowsWidth : 36 }}
          aria-label="Workflows"
        >
          <header className="workflow-pane__header">
            {workflowsOpen ? (
              <span className="workflow-pane__title">Workflows</span>
            ) : (
              <button
                type="button"
                className="workflow-pane__handle"
                aria-expanded={false}
                onClick={() => setWorkflowsOpen(true)}
              >
                Workflows
              </button>
            )}
            <button
              type="button"
              className="workflow-pane__collapse"
              aria-expanded={workflowsOpen}
              title={
                workflowsOpen ? "Collapse workflow list" : "Expand workflow list"
              }
              onClick={() => setWorkflowsOpen((open) => !open)}
            >
              <span aria-hidden="true" className="workflow-pane__chevron">
                »
              </span>
              <span className="sr-only">
                {workflowsOpen ? "Collapse workflow list" : "Expand workflow list"}
              </span>
            </button>
          </header>
          <div className="workflow-pane__body" hidden={!workflowsOpen}>
            <WorkflowPane
              workflows={view.workflows}
              activeIndex={activeWorkflowIndex}
              onSelect={handleSelectWorkflow}
              fetchingWorkflowIndex={fetchingWorkflowIndex}
              onFetchWorkflow={(index) => void handleFetchWorkflow(index)}
            />
          </div>
          {workflowsOpen && (
            <div
              className="workflow-pane__resize"
              onPointerDown={onWorkflowsResizeStart}
              title="Drag to resize"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize workflow list"
            />
          )}
        </section>
        <GraphPane
          view={view}
          workflowIndex={activeWorkflowIndex}
          selection={selection}
          runStateFilters={runStateFilters}
          onToggleRunStateFilter={handleToggleRunStateFilter}
          onSelectJob={handleSelectJob}
          onSelectStep={handleSelectStep}
        />
        <section
          className={`whatif-sheet${sheetOpen ? "" : " whatif-sheet--collapsed"}`}
          style={{ width: sheetOpen ? sheetWidth : 36 }}
          aria-label="What-if"
        >
          {sheetOpen && (
            <div
              className="whatif-sheet__resize"
              onPointerDown={onSheetResizeStart}
              title="Drag to resize"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize what-if panel"
            />
          )}
          <header className="whatif-sheet__header">
            {sheetOpen ? (
              <div className="whatif-sheet__tabs" aria-label="What-if tabs">
                <button
                  type="button"
                  aria-pressed={sheetTab === "context"}
                  aria-controls="whatif-panel-context"
                  id="whatif-tab-context"
                  className="whatif-sheet__tab"
                  onClick={() => setSheetTab("context")}
                >
                  Context
                </button>
                <button
                  type="button"
                  aria-pressed={sheetTab === "selection"}
                  aria-controls="whatif-panel-selection"
                  id="whatif-tab-selection"
                  className="whatif-sheet__tab"
                  onClick={() => setSheetTab("selection")}
                >
                  Selection
                </button>
              </div>
            ) : (
              <button
                type="button"
                className="whatif-sheet__handle"
                aria-expanded={false}
                onClick={() => setSheetOpen(true)}
              >
                What-if
              </button>
            )}
            <button
              type="button"
              className="whatif-sheet__collapse"
              aria-expanded={sheetOpen}
              title={sheetOpen ? "Collapse what-if panel" : "Expand what-if panel"}
              onClick={() => setSheetOpen((open) => !open)}
            >
              <span aria-hidden="true" className="whatif-sheet__chevron">
                »
              </span>
              <span className="sr-only">
                {sheetOpen ? "Collapse what-if panel" : "Expand what-if panel"}
              </span>
            </button>
          </header>
          <div className="whatif-sheet__body" hidden={!sheetOpen}>
            <div
              id="whatif-panel-context"
              className="whatif-sheet__panel"
              hidden={sheetTab !== "context"}
              role="tabpanel"
              aria-labelledby="whatif-tab-context"
            >
              <ContextSheet
                context={view.context}
                workflow={
                  view.workflows.find((w) => w.index === activeWorkflowIndex) ??
                  null
                }
                onApply={handleApplyContext}
                applying={busy && !fetchingAll}
              />
            </div>
            <div
              id="whatif-panel-selection"
              className="whatif-sheet__panel"
              hidden={sheetTab !== "selection"}
              role="tabpanel"
              aria-labelledby="whatif-tab-selection"
            >
              <InspectorPane
                view={view}
                selection={selection}
                onFetchWorkflow={(index) => void handleFetchWorkflow(index)}
                fetchingWorkflowIndex={fetchingWorkflowIndex}
              />
            </div>
          </div>
        </section>
      </main>
      {showBrowser && (
        <PathBrowser
          initialPath={diskPath}
          onOpen={(path) => void handleBrowseOpen(path)}
          onClose={() => setShowBrowser(false)}
        />
      )}
      {editorDiscardPrompt && (
        <div className="scratch-discard" role="alertdialog" aria-labelledby="scratch-discard-title-main">
          <p id="scratch-discard-title-main">
            Workflow YAML has unsaved changes. Close and discard them?
          </p>
          <div className="scratch-discard__actions">
            <button
              type="button"
              className="btn"
              onClick={() => setEditorDiscardPrompt(false)}
            >
              Keep editing
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => closeScratchEditor(true)}
            >
              Discard and close
            </button>
          </div>
        </div>
      )}
      {showEditor && (
        <div
          className="app-drawer"
          style={{ height: drawerHeight }}
        >
          <div
            className="app-drawer__resize"
            onPointerDown={onDrawerResizeStart}
            title="Drag to resize"
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize YAML editor"
          />
          <YamlEditor
            initialSource={editorSource}
            suggestedName={editorName}
            suggestedSavePath={editorSavePath}
            busy={busy}
            onAnalyze={handleAnalyzeSource}
            onSave={handleSaveWorkflow}
            onClose={() => closeScratchEditor(false)}
            onDirtyChange={setEditorDirty}
          />
        </div>
      )}
    </div>
  );
}
