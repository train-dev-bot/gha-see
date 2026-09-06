import type { WebView } from "./types";

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

export async function fetchView(): Promise<WebView> {
  return request<WebView>("/api/view");
}

export async function revaluate(context: import("./types").EvalContextDto): Promise<WebView> {
  return request<WebView>("/api/revaluate", {
    method: "POST",
    body: JSON.stringify(context),
  });
}

export async function fetchRemotes(workflowIndex: number): Promise<WebView> {
  return request<WebView>("/api/fetch", {
    method: "POST",
    body: JSON.stringify({ workflowIndex }),
  });
}

export async function fetchAllRemotes(): Promise<WebView> {
  return request<WebView>("/api/fetch-all", {
    method: "POST",
    body: "{}",
  });
}

export async function analyzePath(path: string): Promise<WebView> {
  return request<WebView>("/api/analyze", {
    method: "POST",
    body: JSON.stringify({ path }),
  });
}

export async function analyzeSource(
  name: string,
  source: string,
): Promise<WebView> {
  return request<WebView>("/api/analyze-source", {
    method: "POST",
    body: JSON.stringify({ name, source }),
  });
}

export async function saveWorkflow(
  path: string,
  content: string,
): Promise<WebView> {
  return request<WebView>("/api/save", {
    method: "POST",
    body: JSON.stringify({ path, content }),
  });
}

export type FsEntryKind = "dir" | "file" | "workflow";

export interface FsEntry {
  name: string;
  path: string;
  kind: FsEntryKind;
}

export interface FsListResponse {
  path: string;
  parent: string | null;
  entries: FsEntry[];
}

export async function listFs(path: string): Promise<FsListResponse> {
  const q = encodeURIComponent(path);
  return request<FsListResponse>(`/api/fs?path=${q}`);
}
