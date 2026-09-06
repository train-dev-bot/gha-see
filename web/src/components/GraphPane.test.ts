import { describe, expect, it } from "vitest";
import type { GraphDto, InstanceDto, WebView } from "../types";
import {
  buildEdges,
  filterVisiblePairs,
  idsMatchingRunState,
  layoutIsolatedNodes,
  withIsolateHidden,
} from "./GraphPane";

const workflowPath = "/repo/.github/workflows/ci.yml";

function instance(instanceId: string): InstanceDto {
  return {
    instanceId,
    baseId: instanceId,
    matrix: {},
    runsOn: null,
    needs: [],
    condition: null,
    outputs: {},
    steps: [],
    support: "supported",
    environment: null,
    concurrency: null,
    services: [],
  };
}

const instances = [instance("build"), instance("test")];
const graph: GraphDto = {
  workflowIndex: 0,
  nodes: ["build", "test"],
  edges: [["build", "test"]],
};
const view = {
  bindings: [
    {
      file: workflowPath,
      consumerJob: "test",
      consumerStepIdx: null,
      producerJob: "build",
      outputName: "artifact",
      raw: "${{ needs.build.outputs.artifact }}",
    },
  ],
} as WebView;
const colors = { needs: "#0ff", dataflow: "#f0f" };

it("returns null when no run-state filters are active", () => {
  expect(idsMatchingRunState(view, workflowPath, ["build", "test"], new Set())).toBeNull();
});

it("keeps only jobs whose run-state is selected", () => {
  const filteredView = {
    ...view,
    jobStates: [
      { file: workflowPath, instanceId: "build", state: "willRun" },
      { file: workflowPath, instanceId: "test", state: "skipped" },
    ],
  } as WebView;
  expect(
    [...idsMatchingRunState(filteredView, workflowPath, ["build", "test"], new Set(["skipped"]))!].sort(),
  ).toEqual(["test"]);
});

describe("buildEdges visibility", () => {
  it("omits depends-on edges when needs lines are hidden", () => {
    const edges = buildEdges(view, graph, workflowPath, instances, null, colors, {
      showNeeds: false,
      showOutputs: true,
    });

    expect(edges.map((edge) => edge.data?.kind)).toEqual(["dataflow"]);
  });

  it("omits dataflow edges when output lines are hidden", () => {
    const edges = buildEdges(view, graph, workflowPath, instances, null, colors, {
      showNeeds: true,
      showOutputs: false,
    });

    expect(edges.map((edge) => edge.data?.kind)).toEqual(["needs"]);
  });

  it("omits edges that touch jobs outside the isolate set", () => {
    const edges = buildEdges(
      view,
      graph,
      workflowPath,
      instances,
      null,
      colors,
      { showNeeds: true, showOutputs: true },
      new Set(["build"]),
    );
    expect(edges).toEqual([]);
  });

  it("keeps isolate edges whose both ends are visible", () => {
    const edges = buildEdges(
      view,
      graph,
      workflowPath,
      instances,
      null,
      colors,
      { showNeeds: true, showOutputs: true },
      new Set(["build", "test"]),
    );
    expect(edges.map((edge) => edge.id).sort()).toEqual([
      "dataflow-build-test-artifact-0",
      "needs-build-test",
    ]);
  });
});

describe("isolate layout helpers", () => {
  it("keeps only pairs whose both ends are visible", () => {
    const visible = new Set(["build", "test"]);
    expect(
      filterVisiblePairs(
        [
          ["build", "test"],
          ["test", "deploy"],
          ["lint", "build"],
        ],
        visible,
      ),
    ).toEqual([["build", "test"]]);
  });

  it("hides nodes outside the isolate set", () => {
    const flagged = withIsolateHidden(
      [{ id: "build" }, { id: "test" }, { id: "deploy" }],
      new Set(["build", "test"]),
    );
    expect(flagged.map((n) => [n.id, n.hidden])).toEqual([
      ["build", false],
      ["test", false],
      ["deploy", true],
    ]);
  });

  it("layouts only visible isolate ids", () => {
    const heights = new Map([
      ["lint", 100],
      ["build", 100],
      ["test", 100],
    ]);
    const laid = layoutIsolatedNodes(
      ["build", "test"],
      [
        ["lint", "build"],
        ["build", "test"],
      ],
      heights,
      new Map(),
      [
        ["lint", "build"],
        ["build", "test"],
      ],
    );
    expect([...laid.keys()].sort()).toEqual(["build", "test"]);
  });
});
