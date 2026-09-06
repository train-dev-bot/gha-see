import { useCallback, useEffect, useMemo, useState } from "react";
import PathBrowser, { workflowFileName } from "./PathBrowser";

const EMPTY_TEMPLATE = `name: Scratch workflow
on:
  push:
    branches: [main]
  workflow_dispatch:
    inputs:
      environment:
        description: Target environment
        default: production

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Hello
        run: echo "paste your workflow here"
`;

interface YamlEditorProps {
  initialSource: string;
  suggestedName: string;
  suggestedSavePath: string;
  busy: boolean;
  onAnalyze: (name: string, source: string) => Promise<void>;
  onSave: (path: string, source: string) => Promise<void>;
  onClose: () => void;
  onDirtyChange?: (dirty: boolean) => void;
}

interface HistoryState {
  stack: string[];
  idx: number;
}

export default function YamlEditor({
  initialSource,
  suggestedName,
  suggestedSavePath,
  busy,
  onAnalyze,
  onSave,
  onClose,
  onDirtyChange,
}: YamlEditorProps) {
  const [savePath, setSavePath] = useState(suggestedSavePath);
  const [baseline, setBaseline] = useState(initialSource || EMPTY_TEMPLATE);
  const [history, setHistory] = useState<HistoryState>(() => ({
    stack: [initialSource || EMPTY_TEMPLATE],
    idx: 0,
  }));
  const [error, setError] = useState<string | null>(null);
  const [showBrowser, setShowBrowser] = useState(false);

  const source = history.stack[history.idx] ?? "";

  useEffect(() => {
    // Reset history when opening a different document
    const next = initialSource || EMPTY_TEMPLATE;
    setHistory({ stack: [next], idx: 0 });
    setBaseline(next);
    setSavePath(suggestedSavePath);
    setError(null);
    setShowBrowser(false);
  }, [initialSource, suggestedName, suggestedSavePath]);

  const push = useCallback((next: string) => {
    setHistory((prev) => {
      const trimmed = prev.stack.slice(0, prev.idx + 1);
      const last = trimmed[trimmed.length - 1];
      if (last === next) return { stack: trimmed, idx: trimmed.length - 1 };
      const merged = [...trimmed, next];
      // Cap undo stack
      const stack = merged.length > 80 ? merged.slice(merged.length - 80) : merged;
      return { stack, idx: stack.length - 1 };
    });
  }, []);

  const onChange = (value: string) => {
    push(value);
  };

  const undo = () => {
    setHistory((prev) => ({ ...prev, idx: Math.max(0, prev.idx - 1) }));
  };

  const redo = () => {
    setHistory((prev) => ({
      ...prev,
      idx: Math.min(prev.stack.length - 1, prev.idx + 1),
    }));
  };

  const canUndo = history.idx > 0;
  const canRedo = history.idx < history.stack.length - 1;

  const dirty = useMemo(() => source !== baseline, [source, baseline]);
  const fileName = workflowFileName(savePath, suggestedName);

  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);

  const runAnalyze = async () => {
    setError(null);
    try {
      await onAnalyze(fileName, source);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const runSave = async () => {
    setError(null);
    try {
      await onSave(savePath.trim(), source);
      setBaseline(source);
      setHistory({ stack: [source], idx: 0 });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="yaml-editor">
      <div className="yaml-editor__toolbar">
        <div className="yaml-editor__title">
          <strong>Workflow YAML</strong>
          <span>
            View or edit the active workflow, analyze without writing disk, then
            save if you want
            {dirty ? " · unsaved changes" : ""}
          </span>
        </div>
        <div className="yaml-editor__actions">
          <button type="button" className="btn" disabled={!canUndo || busy} onClick={undo}>
            Undo
          </button>
          <button type="button" className="btn" disabled={!canRedo || busy} onClick={redo}>
            Redo
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || !source.trim()}
            onClick={() => void runAnalyze()}
          >
            {busy ? "Analyzing…" : "Analyze"}
          </button>
          <button type="button" className="btn" disabled={busy} onClick={onClose}>
            Close
          </button>
        </div>
      </div>

      <div className="yaml-editor__meta">
        <label className="context-field yaml-editor__save-path">
          <span>Save as</span>
          <div className="yaml-editor__save-row">
            <input
              className="input mono"
              value={savePath}
              onChange={(e) => setSavePath(e.target.value)}
              spellCheck={false}
              placeholder="./.github/workflows/scratch.yml"
            />
            <button
              type="button"
              className="btn"
              disabled={busy}
              onClick={() => setShowBrowser(true)}
            >
              Browse
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={busy || !savePath.trim() || !source.trim()}
              onClick={() => void runSave()}
            >
              Save
            </button>
          </div>
        </label>
      </div>

      {error && <div className="error-banner">{error}</div>}
      {dirty && (
        <div className="yaml-editor__dirty" role="status">
          Unsaved changes — Analyze keeps them in memory; Save writes to disk.
        </div>
      )}

      <div className="yaml-editor__body">
        <textarea
          className="yaml-editor__textarea mono"
          value={source}
          onChange={(e) => onChange(e.target.value)}
          spellCheck={false}
          placeholder="Paste workflow YAML here…"
        />
      </div>
      {showBrowser && (
        <PathBrowser
          mode="save"
          fileName={fileName}
          initialPath={savePath}
          onOpen={(path) => {
            setSavePath(path);
            setShowBrowser(false);
          }}
          onClose={() => setShowBrowser(false)}
        />
      )}
    </div>
  );
}

export { EMPTY_TEMPLATE };
