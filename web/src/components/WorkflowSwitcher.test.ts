import { describe, expect, it } from "vitest";
import { filterWorkflows } from "./WorkflowSwitcher";
import type { WorkflowDto } from "../types";

function wf(index: number, name: string | null, path: string): WorkflowDto {
  return {
    index,
    path,
    name,
    parseOk: true,
    fileError: null,
    rawSource: null,
    hasRemoteUses: false,
    needsRemoteFetch: false,
    instances: [],
    dispatchInputs: [],
  };
}

describe("filterWorkflows", () => {
  const list = [
    wf(0, "Happy path", "/repo/.github/workflows/01_happy_path.yml"),
    wf(1, "CI/CD Showcase (TypeScript)", "/repo/.github/workflows/11_cicd_typescript.yml"),
  ];

  it("returns all workflows when the query is empty", () => {
    expect(filterWorkflows(list, "  ").map((w) => w.index)).toEqual([0, 1]);
  });

  it("matches name or filename case-insensitively", () => {
    expect(filterWorkflows(list, "type").map((w) => w.index)).toEqual([1]);
    expect(filterWorkflows(list, "HAPPY").map((w) => w.index)).toEqual([0]);
    expect(filterWorkflows(list, "01_happy").map((w) => w.index)).toEqual([0]);
  });
});
