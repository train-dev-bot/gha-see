import ThemeSwitcher from "./ThemeSwitcher";

export interface AppHeaderProps {
  pathLabel: string;
  showScratchBadge: boolean;
  reloading: boolean;
  issueCount: number;
  skippedCount: number;
  pendingFetchCount: number;
  fetchingAll: boolean;
  showEditor: boolean;
  editorDirty: boolean;
  onReload: () => void;
  onBrowse: () => void;
  onFetchAll: () => void;
  onToggleEditor: () => void;
}

export default function AppHeader({
  pathLabel,
  showScratchBadge,
  reloading,
  issueCount,
  skippedCount,
  pendingFetchCount,
  fetchingAll,
  showEditor,
  editorDirty,
  onReload,
  onBrowse,
  onFetchAll,
  onToggleEditor,
}: AppHeaderProps) {
  return (
    <header className="app-header">
      <div className="app-header__left">
        <strong>gha-see</strong>
        <div className="path-chrome">
          <button
            type="button"
            className="path-chrome__reload"
            title="Reload"
            disabled={reloading}
            onClick={onReload}
          >
            ↻
          </button>
          <button
            type="button"
            className="path-chrome__path mono"
            title="Open a folder of workflows…"
            onClick={onBrowse}
          >
            {pathLabel}
          </button>
        </div>
        {showScratchBadge && (
          <span
            className="tree-badge tree-badge--fetch"
            title="Showing in-memory scratch analysis"
          >
            scratch
          </span>
        )}
      </div>
      <div className="app-header__right" aria-label="Analysis status">
        <ThemeSwitcher />
        <span className="app-header__stat">
          {issueCount} issue{issueCount === 1 ? "" : "s"}
        </span>
        <span className="app-header__stat app-header__stat--muted">
          {skippedCount} skipped
        </span>
        <span
          className={
            pendingFetchCount > 0
              ? "app-header__stat app-header__pending"
              : "app-header__stat"
          }
        >
          {pendingFetchCount} remote{pendingFetchCount === 1 ? "" : "s"} to fetch
        </span>
        <button
          type="button"
          className="btn btn-fetch"
          disabled={pendingFetchCount === 0 || fetchingAll}
          onClick={onFetchAll}
          title="Download missing remote actions into ~/.cache/gha-see (explicit network)"
        >
          {fetchingAll ? "Fetching…" : "Fetch all"}
        </button>
        <button
          type="button"
          className="btn btn-primary"
          onClick={onToggleEditor}
          aria-pressed={showEditor}
          title={
            showEditor
              ? "Hide workflow YAML editor"
              : "View & edit workflow YAML"
          }
        >
          View & edit YAML{editorDirty ? " · unsaved" : ""}
        </button>
      </div>
    </header>
  );
}
