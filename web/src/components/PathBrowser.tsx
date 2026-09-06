import { useCallback, useEffect, useState } from "react";
import { listFs, type FsEntry, type FsListResponse } from "../api";

interface PathBrowserProps {
  initialPath: string;
  onOpen: (path: string) => void;
  onClose: () => void;
  mode?: "open" | "save";
  fileName?: string;
}

export function joinSave(path: string, fileName: string) {
  const directory = path.replace(/\/+$/, "");
  const name = fileName.replace(/^\/+/, "");
  return directory ? `${directory}/${name}` : `/${name}`;
}

export function savePathForChoice(
  path: string,
  listingPath: string | undefined,
  fileName: string | undefined,
) {
  const chosen = path.trim();
  if (chosen === listingPath || !/\.ya?ml$/i.test(chosen)) {
    return joinSave(chosen, fileName ?? "scratch.yml");
  }
  return chosen;
}

export function dirnamePath(path: string) {
  const lastSlash = path.lastIndexOf("/");
  if (lastSlash < 0) return ".";
  if (lastSlash === 0) return "/";
  return path.slice(0, lastSlash);
}

export function workflowFileName(path: string, fallback = "scratch.yml") {
  const base = path.replace(/\\/g, "/").replace(/\/+$/, "").split("/").pop() ?? "";
  return /\.ya?ml$/i.test(base) ? base : fallback;
}

/** Browse always loads a folder. A YAML click still analyzes that folder, then selects the file. */
export function openFolderPath(path: string): {
  folder: string;
  selectFile: string | null;
} {
  const chosen = path.trim();
  if (/\.ya?ml$/i.test(chosen)) {
    return { folder: dirnamePath(chosen), selectFile: chosen };
  }
  return { folder: chosen, selectFile: null };
}

export function workflowIndexForPath(
  workflows: { index: number; path: string }[],
  filePath: string,
): number | undefined {
  const norm = filePath.replace(/\\/g, "/");
  const base = norm.split("/").pop() ?? norm;
  const exact = workflows.find((w) => w.path.replace(/\\/g, "/") === norm);
  if (exact) return exact.index;
  return workflows.find(
    (w) => (w.path.replace(/\\/g, "/").split("/").pop() ?? w.path) === base,
  )?.index;
}

export function initialBrowsePath(path: string) {
  return /\.ya?ml$/i.test(path) ? dirnamePath(path) : path;
}

export default function PathBrowser({
  initialPath,
  onOpen,
  onClose,
  mode = "open",
  fileName,
}: PathBrowserProps) {
  const [listing, setListing] = useState<FsListResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [manual, setManual] = useState(initialPath);

  const load = useCallback(async (path: string) => {
    setLoading(true);
    setError(null);
    try {
      const target = initialBrowsePath(path);
      let data: FsListResponse;
      try {
        data = await listFs(target);
      } catch (e) {
        const parent = dirnamePath(target);
        if (
          parent === target ||
          !(e instanceof Error) ||
          !e.message.includes("path is not a directory")
        ) {
          throw e;
        }
        data = await listFs(parent);
      }
      setListing(data);
      setManual(data.path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(initialPath || ".");
  }, [initialPath, load]);

  const onEntry = (entry: FsEntry) => {
    if (entry.kind === "dir") {
      void load(entry.path);
      return;
    }
    if (entry.kind === "workflow") {
      onOpen(entry.path);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose} role="presentation">
      <div
        className="modal path-browser"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label={mode === "save" ? "Save workflow path" : "Open workflow folder"}
      >
        <div className="modal__header">
          <h3>{mode === "save" ? "Save workflow" : "Open folder"}</h3>
          <button type="button" className="btn" onClick={onClose}>
            Close
          </button>
        </div>

        <div className="path-browser__bar">
          {listing?.parent != null && (
            <button
              type="button"
              className="btn"
              disabled={loading}
              onClick={() => void load(listing.parent!)}
            >
              Up
            </button>
          )}
          <input
            className="input mono"
            value={manual}
            aria-label="Folder path"
            onChange={(e) => setManual(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void load(manual);
            }}
            spellCheck={false}
          />
          <button
            type="button"
            className="btn btn-primary"
            disabled={loading || !manual.trim()}
            onClick={() => {
              const chosen = manual.trim();
              onOpen(
                mode === "save"
                  ? savePathForChoice(chosen, listing?.path, fileName)
                  : chosen,
              );
            }}
          >
            {mode === "save" ? "Save here" : "Open folder"}
          </button>
        </div>

        {error && <div className="error-banner">{error}</div>}

        <div className="path-browser__list">
          {loading && <p className="inspector-hint">Loading…</p>}
          {!loading &&
            listing?.entries.map((entry) => (
              <button
                key={entry.path}
                type="button"
                className={`path-browser__row path-browser__row--${entry.kind}`}
                onClick={() => onEntry(entry)}
                onDoubleClick={() => {
                  if (entry.kind === "workflow" || entry.kind === "dir") {
                    if (entry.kind === "dir") void load(entry.path);
                    else onOpen(entry.path);
                  }
                }}
              >
                <span className="path-browser__kind">{entry.kind}</span>
                <span className="path-browser__name">{entry.name}</span>
              </button>
            ))}
          {!loading && listing && listing.entries.length === 0 && (
            <p className="inspector-hint">Empty directory</p>
          )}
        </div>

        {mode === "save" ? (
          <p className="context-hint path-browser__hint">
            Enter goes to a folder. Save here writes{" "}
            <code>{fileName ?? "scratch.yml"}</code> into this folder. Click an
            existing workflow to overwrite it.
          </p>
        ) : (
          <p className="context-hint path-browser__hint">
            Enter goes to a folder. Open this folder to load every workflow.
            Click a <code>.yml</code> to open its folder and select that file.
          </p>
        )}
      </div>
    </div>
  );
}
