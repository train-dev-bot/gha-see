import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  MarkerType,
  Panel,
  useNodesState,
  useEdgesState,
  useReactFlow,
  useNodesInitialized,
  ReactFlowProvider,
  type Node,
  type Edge,
  type NodeTypes,
  type EdgeTypes,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import JobNode from "./JobNode";
import EnvBandsOverlay from "./EnvBandsOverlay";
import FlowEdgeComponent, { type FlowEdgeData } from "./FlowEdge";
import {
  estimateNodeHeight,
  layoutNodes,
  NODE_WIDTH,
  GAP_Y,
  nudgeDroppedBox,
  packStackedGaps,
} from "../layout";
import { applyBandEviction, boxesFromNodes } from "../envBands";
import type {
  BindingDto,
  GraphDto,
  InstanceDto,
  JobNodeData,
  RunState,
  Selection,
  WebView,
} from "../types";
import {
  ALL_RUN_STATES,
  RUN_STATE_LABELS,
  jobState,
  resolveInstanceId,
  stepState,
} from "../types";
import { useThemeColor } from "../themes";

const nodeTypes: NodeTypes = {
  job: JobNode,
};

const edgeTypes: EdgeTypes = {
  flow: FlowEdgeComponent,
};

/** Needs + dataflow pairs for widening busy column gutters. */
function densityEdgePairs(
  graph: GraphDto,
  view: WebView,
  workflowPath: string,
  instances: InstanceDto[],
): [string, string][] {
  const pairs: [string, string][] = [...graph.edges];
  for (const b of view.bindings) {
    if (b.file !== workflowPath) continue;
    const source = resolveInstanceId(instances, b.producerJob);
    const target = resolveInstanceId(instances, b.consumerJob);
    if (source && target) pairs.push([source, target]);
  }
  return pairs;
}

export function filterVisiblePairs(
  pairs: [string, string][],
  visible: Set<string>,
): [string, string][] {
  return pairs.filter(([from, to]) => visible.has(from) && visible.has(to));
}

export function withIsolateHidden<T extends { id: string }>(
  nodes: T[],
  visible: Set<string>,
): Array<T & { hidden: boolean }> {
  return nodes.map((node) => ({
    ...node,
    hidden: !visible.has(node.id),
  }));
}

export function idsMatchingRunState(
  view: WebView,
  file: string,
  instanceIds: string[],
  filters: Set<RunState>,
): Set<string> | null {
  if (filters.size === 0) return null;
  return new Set(
    instanceIds.filter((id) => filters.has(jobState(view, file, id))),
  );
}

export function layoutIsolatedNodes(
  visibleIds: string[],
  edges: [string, string][],
  heights?: Map<string, number>,
  environments?: Map<string, string | null | undefined>,
  densityPairs?: [string, string][],
): Map<string, { x: number; y: number }> {
  const visible = new Set(visibleIds);
  return layoutNodes(
    visibleIds,
    filterVisiblePairs(edges, visible),
    heights,
    environments,
    densityPairs ? filterVisiblePairs(densityPairs, visible) : undefined,
  );
}

/** User-dragged positions only — survives GraphCanvas remounts. Keyed by path. */
const SESSION_LAYOUTS = new Map<string, Map<string, { x: number; y: number }>>();

interface GraphPaneProps {
  view: WebView;
  workflowIndex: number;
  selection: Selection | null;
  runStateFilters: Set<RunState>;
  onToggleRunStateFilter: (state: RunState) => void;
  onSelectJob: (workflowIndex: number, instanceId: string, file: string) => void;
  onSelectStep: (
    workflowIndex: number,
    instanceId: string,
    file: string,
    stepIdx: number,
  ) => void;
}

/** Jobs + edges that stay bright after an edge click. */
type EdgeFocus =
  | {
      kind: "dataflow";
      outputName: string;
      nodes: Set<string>;
      primaryEdgeId: string;
    }
  | {
      kind: "needs";
      nodes: Set<string>;
      edgeIds: Set<string>;
      primaryEdgeId: string;
    };

function walkDirected(
  edges: [string, string][],
  start: string,
  forward: boolean,
): Set<string> {
  const adj = new Map<string, string[]>();
  for (const [a, b] of edges) {
    const from = forward ? a : b;
    const to = forward ? b : a;
    if (!adj.has(from)) adj.set(from, []);
    adj.get(from)!.push(to);
  }
  const out = new Set<string>();
  const stack = [start];
  while (stack.length > 0) {
    const n = stack.pop()!;
    for (const next of adj.get(n) ?? []) {
      if (out.has(next)) continue;
      out.add(next);
      stack.push(next);
    }
  }
  return out;
}

function focusForNeedsEdge(
  graphEdges: [string, string][],
  edge: Edge<FlowEdgeData>,
): EdgeFocus {
  // Parents only: ancestors of the source + the clicked hop. No downstream fan-out.
  const nodes = new Set<string>([
    ...walkDirected(graphEdges, edge.source, false),
    edge.source,
    edge.target,
  ]);
  const edgeIds = new Set<string>();
  for (const [from, to] of graphEdges) {
    if (nodes.has(from) && nodes.has(to)) {
      edgeIds.add(`needs-${from}-${to}`);
    }
  }
  return { kind: "needs", nodes, edgeIds, primaryEdgeId: edge.id };
}

function focusForDataflowEdge(
  view: WebView,
  workflowPath: string,
  instances: InstanceDto[],
  edge: Edge<FlowEdgeData>,
): EdgeFocus {
  const outputName = edge.data?.outputName ?? "";
  const nodes = new Set<string>([edge.source, edge.target]);

  if (outputName) {
    for (const b of view.bindings) {
      if (b.file !== workflowPath || b.outputName !== outputName) continue;
      const source = resolveInstanceId(instances, b.producerJob);
      const target = resolveInstanceId(instances, b.consumerJob);
      if (source) nodes.add(source);
      if (target) nodes.add(target);
    }
  }

  return {
    kind: "dataflow",
    outputName,
    nodes,
    primaryEdgeId: edge.id,
  };
}

function assignSlots(
  pairs: Array<{ key: string }>,
): Map<number, { slot: number; peerCount: number }> {
  const counts = new Map<string, number>();
  for (const p of pairs) {
    counts.set(p.key, (counts.get(p.key) ?? 0) + 1);
  }
  const seen = new Map<string, number>();
  const out = new Map<number, { slot: number; peerCount: number }>();
  pairs.forEach((p, i) => {
    const slot = seen.get(p.key) ?? 0;
    seen.set(p.key, slot + 1);
    out.set(i, { slot, peerCount: counts.get(p.key) ?? 1 });
  });
  return out;
}

export function buildEdges(
  view: WebView,
  graph: GraphDto,
  workflowPath: string,
  instances: InstanceDto[],
  edgeFocus: EdgeFocus | null,
  colors: { needs: string; dataflow: string },
  visibility: { showNeeds: boolean; showOutputs: boolean },
  /** When set (isolate), drop edges that touch a hidden job. */
  visibleIds?: Set<string> | null,
): Edge<FlowEdgeData>[] {
  const instanceMap = new Map(instances.map((i) => [i.instanceId, i]));
  type Draft = { key: string; edge: Edge<FlowEdgeData> };
  const drafts: Draft[] = [];

  for (const [from, to] of graph.edges) {
    drafts.push({
      key: `${from}→${to}`,
      edge: {
        id: `needs-${from}-${to}`,
        source: from,
        target: to,
        sourceHandle: "job-out",
        targetHandle: "job-in",
        type: "flow",
        label: "depends on",
        markerEnd: {
          type: MarkerType.ArrowClosed,
          width: 16,
          height: 16,
          color: colors.needs,
        },
        data: { kind: "needs", slot: 0, peerCount: 1 },
      },
    });
  }

  view.bindings
    .filter((b) => b.file === workflowPath)
    .forEach((b: BindingDto, i) => {
      const source = resolveInstanceId(instances, b.producerJob);
      const target = resolveInstanceId(instances, b.consumerJob);
      if (!source || !target) return;
      if (!instanceMap.has(source) || !instanceMap.has(target)) return;
      const stepIdx = b.consumerStepIdx;
      const targetHandle =
        stepIdx != null ? `step-${stepIdx}-in` : "job-in";
      const stepLabel =
        stepIdx != null
          ? instances
              .find((inst) => inst.instanceId === target)
              ?.steps.find((s) => s.index === stepIdx)
          : null;
      const stepName =
        stepLabel?.name ??
        stepLabel?.id ??
        (stepIdx != null ? `step ${stepIdx + 1}` : null);
      const label = stepName ? `${b.outputName} → ${stepName}` : b.outputName;
      drafts.push({
        key: `${source}→${target}`,
        edge: {
          id: `dataflow-${source}-${target}-${b.outputName}-${i}`,
          source,
          target,
          sourceHandle: "job-out",
          targetHandle,
          type: "flow",
          label,
          markerEnd: {
            type: MarkerType.ArrowClosed,
            width: 14,
            height: 14,
            color: colors.dataflow,
          },
          data: {
            kind: "dataflow",
            slot: 0,
            peerCount: 1,
            outputName: b.outputName,
          },
        },
      });
    });

  const visibleDrafts = drafts.filter(({ edge }) => {
    if (visibility.showNeeds === false && edge.data?.kind === "needs") return false;
    if (visibility.showOutputs === false && edge.data?.kind === "dataflow") {
      return false;
    }
    if (visibleIds && (!visibleIds.has(edge.source) || !visibleIds.has(edge.target))) {
      return false;
    }
    return true;
  });
  const slots = assignSlots(visibleDrafts.map((d) => ({ key: d.key })));
  return visibleDrafts.map((d, i) => {
    const { slot, peerCount } = slots.get(i)!;
    const focused =
      edgeFocus != null &&
      (edgeFocus.kind === "dataflow"
        ? d.edge.data?.kind === "dataflow" &&
          d.edge.data.outputName === edgeFocus.outputName
        : edgeFocus.edgeIds.has(d.edge.id));
    const dimmed = edgeFocus != null && !focused;
    return {
      ...d.edge,
      selected: focused && d.edge.id === edgeFocus?.primaryEdgeId,
      data: {
        ...d.edge.data!,
        kind: d.edge.data!.kind,
        slot,
        peerCount,
        dimmed,
      },
    };
  });
}

function buildNodeData(
  view: WebView,
  workflowPath: string,
  workflowIndex: number,
  selection: Selection | null,
  inst: InstanceDto,
  onSelectJob: GraphPaneProps["onSelectJob"],
  onSelectStep: GraphPaneProps["onSelectStep"],
  edgeFocus: EdgeFocus | null,
): JobNodeData {
  const id = inst.instanceId;
  const stepStates = new Map(
    inst.steps.map((s) => [
      s.index,
      stepState(view, workflowPath, id, s.index),
    ]),
  );
  const selected =
    (selection?.kind === "job" || selection?.kind === "step") &&
    selection.workflowIndex === workflowIndex &&
    selection.instanceId === id;
  const selectedStepIdx =
    selection?.kind === "step" &&
    selection.workflowIndex === workflowIndex &&
    selection.instanceId === id
      ? selection.stepIdx
      : null;
  const dimmed = edgeFocus != null && !edgeFocus.nodes.has(id);

  return {
    instanceId: id,
    file: workflowPath,
    workflowIndex,
    runsOn: inst.runsOn,
    runState: jobState(view, workflowPath, id),
    steps: inst.steps,
    stepStates,
    selected: !!selected,
    selectedStepIdx,
    dimmed,
    support: inst.support,
    condition: inst.condition,
    needs: inst.needs,
    environmentName: inst.environment?.name ?? null,
    onSelectJob,
    onSelectStep,
  };
}

type IsolateSnap = {
  positions: Map<string, { x: number; y: number }>;
  viewport: { x: number; y: number; zoom: number };
};

function GraphCanvas({
  view,
  workflowIndex,
  selection,
  runStateFilters,
  onToggleRunStateFilter,
  onSelectJob,
  onSelectStep,
}: GraphPaneProps) {
  const graph = view.graphs.find((g) => g.workflowIndex === workflowIndex);
  const wf = view.workflows.find((w) => w.index === workflowIndex);
  const workflowPath = wf?.path ?? "";
  const instances = wf?.instances ?? [];
  const { fitView, getNodes, getViewport, setViewport } = useReactFlow();
  const nodesInitialized = useNodesInitialized();
  const [edgeFocus, setEdgeFocus] = useState<EdgeFocus | null>(null);
  const [showNeeds, setShowNeeds] = useState(true);
  const [showOutputs, setShowOutputs] = useState(true);
  const [isolateMode, setIsolateMode] = useState(false);
  const isolateVisibleIds =
    isolateMode && edgeFocus != null ? edgeFocus.nodes : null;
  const runVisibleIds = useMemo(
    () =>
      idsMatchingRunState(
        view,
        workflowPath,
        graph?.nodes ?? [],
        runStateFilters,
      ),
    [graph, runStateFilters, view, workflowPath],
  );
  const visibleIds = useMemo(() => {
    if (isolateVisibleIds == null) return runVisibleIds;
    if (runVisibleIds == null) return isolateVisibleIds;
    return new Set(
      [...isolateVisibleIds].filter((id) => runVisibleIds.has(id)),
    );
  }, [isolateVisibleIds, runVisibleIds]);
  const isolateSnapRef = useRef<IsolateSnap | null>(null);
  const isolateFitIdsRef = useRef<string[] | null>(null);
  const isolateFitGenerationRef = useRef(0);
  const [isolateFitNonce, setIsolateFitNonce] = useState(0);
  const needsColor = useThemeColor("--cyan", "#449dab");
  const dataflowColor = useThemeColor("--magenta", "#ad8ee6");
  const minimapMask = useThemeColor("--bg", "#1a1b26");
  const edgeColors = useMemo(
    () => ({ needs: needsColor, dataflow: dataflowColor }),
    [needsColor, dataflowColor],
  );

  const positionsRef = useRef<Map<string, { x: number; y: number }>>(new Map());
  const layoutEpochRef = useRef<string>("");
  /** Center once nodes have measured dimensions (not on user-restored layouts). */
  const wantCenterRef = useRef(false);
  /** Bumps when we request a center so the effect can re-run after StrictMode cleanup. */
  const [centerNonce, setCenterNonce] = useState(0);
  /** Last pane size used for fitView — refit when the flex panel settles on first load. */
  const paneSizeRef = useRef({ w: 0, h: 0 });
  const flowRootRef = useRef<HTMLDivElement | null>(null);
  /** Pack stacked cards once per layout using measured heights. */
  const packedEpochRef = useRef<string>("");
  const skipPackRef = useRef(false);

  const structureOnlyKey = useMemo(() => {
    if (!graph) return `${workflowPath}:empty`;
    return [
      workflowPath,
      graph.nodes.join("\0"),
      graph.edges.map(([a, b]) => `${a}>${b}`).join("\0"),
      view.bindings
        .filter((b) => b.file === workflowPath)
        .map((b) => `${b.producerJob}:${b.consumerJob}:${b.outputName}`)
        .join("\0"),
    ].join("|");
  }, [graph, workflowPath, view.bindings]);

  const structureKey = useMemo(() => {
    return `${workflowIndex}|${structureOnlyKey}`;
  }, [workflowIndex, structureOnlyKey]);

  const initialLayout = useMemo(() => {
    if (!graph) return new Map<string, { x: number; y: number }>();
    const heights = new Map<string, number>();
    const environments = new Map<string, string | null>();
    for (const inst of instances) {
      heights.set(inst.instanceId, estimateNodeHeight(inst.steps.length));
      environments.set(inst.instanceId, inst.environment?.name ?? null);
    }
    const laid = layoutNodes(
      graph.nodes,
      graph.edges,
      heights,
      environments,
      densityEdgePairs(graph, view, workflowPath, instances),
    );
    const boxes = instances.map((inst) => ({
      id: inst.instanceId,
      env: inst.environment?.name ?? "",
      x: laid.get(inst.instanceId)?.x ?? 0,
      y: laid.get(inst.instanceId)?.y ?? 0,
      width: NODE_WIDTH,
      height: heights.get(inst.instanceId) ?? estimateNodeHeight(3),
    }));
    return applyBandEviction(new Map(laid), boxes, graph.edges);
  }, [graph, instances, view, workflowPath]);

  const seedNodes = useMemo((): Node<JobNodeData>[] => {
    if (!graph) return [];
    return graph.nodes
      .map((id) => instances.find((i) => i.instanceId === id))
      .filter((inst): inst is InstanceDto => !!inst)
      .map((inst) => {
        const data = buildNodeData(
          view,
          workflowPath,
          workflowIndex,
          selection,
          inst,
          onSelectJob,
          onSelectStep,
          edgeFocus,
        );
        return {
          id: inst.instanceId,
          type: "job" as const,
          position:
            positionsRef.current.get(inst.instanceId) ??
            initialLayout.get(inst.instanceId) ?? { x: 0, y: 0 },
          data,
          draggable: !data.dimmed,
        };
      });
  }, [
    graph,
    instances,
    initialLayout,
    view,
    workflowPath,
    workflowIndex,
    selection,
    onSelectJob,
    onSelectStep,
    edgeFocus,
  ]);

  const seedEdges = useMemo(() => {
    if (!graph) return [];
    return buildEdges(
      view,
      graph,
      workflowPath,
      instances,
      edgeFocus,
      edgeColors,
      {
        showNeeds,
        showOutputs,
      },
      visibleIds,
    );
  }, [
    graph,
    view,
    workflowPath,
    instances,
    edgeFocus,
    edgeColors,
    showNeeds,
    showOutputs,
    visibleIds,
  ]);

  const [nodes, setNodes, onNodesChange] = useNodesState(seedNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(seedEdges);

  // Full layout when structure changes. Only restore SESSION_LAYOUTS after the user
  // has dragged — auto-layout is never cached (avoids StrictMode skipping fitView).
  useEffect(() => {
    if (structureKey === layoutEpochRef.current) return;
    layoutEpochRef.current = structureKey;

    const userLayout = SESSION_LAYOUTS.get(workflowPath);
    const hasUserLayout = Boolean(userLayout && userLayout.size > 0);
    skipPackRef.current = hasUserLayout;
    packedEpochRef.current = "";
    isolateSnapRef.current = null;
    isolateFitIdsRef.current = null;
    if (hasUserLayout) {
      positionsRef.current = new Map(userLayout);
    } else {
      positionsRef.current = new Map();
    }

    if (!graph) {
      setNodes([]);
      setEdges([]);
      wantCenterRef.current = false;
      return;
    }

    const heights = new Map<string, number>();
    const environments = new Map<string, string | null>();
    for (const inst of instances) {
      heights.set(inst.instanceId, estimateNodeHeight(inst.steps.length));
      environments.set(inst.instanceId, inst.environment?.name ?? null);
    }
    const laid = layoutNodes(
      graph.nodes,
      graph.edges,
      heights,
      environments,
      densityEdgePairs(graph, view, workflowPath, instances),
    );
    for (const [id, pos] of laid) {
      if (!positionsRef.current.has(id)) {
        positionsRef.current.set(id, pos);
      }
    }
    for (const id of [...positionsRef.current.keys()]) {
      if (!laid.has(id)) positionsRef.current.delete(id);
    }

    const boxes = instances.map((inst) => {
      const pos = positionsRef.current.get(inst.instanceId) ??
        laid.get(inst.instanceId) ?? { x: 0, y: 0 };
      return {
        id: inst.instanceId,
        env: inst.environment?.name ?? "",
        x: pos.x,
        y: pos.y,
        width: NODE_WIDTH,
        height: heights.get(inst.instanceId) ?? estimateNodeHeight(3),
      };
    });
    applyBandEviction(positionsRef.current, boxes, graph.edges);

    setEdgeFocus(null);
    setNodes(
      graph.nodes
        .map((id) => instances.find((i) => i.instanceId === id))
        .filter((inst): inst is InstanceDto => !!inst)
        .map((inst) => ({
          id: inst.instanceId,
          type: "job" as const,
          draggable: true,
          position: positionsRef.current.get(inst.instanceId) ??
            laid.get(inst.instanceId) ?? { x: 0, y: 0 },
          data: buildNodeData(
            view,
            workflowPath,
            workflowIndex,
            selection,
            inst,
            onSelectJob,
            onSelectStep,
            null,
          ),
        })),
    );
    setEdges(
      buildEdges(view, graph, workflowPath, instances, null, edgeColors, {
        showNeeds,
        showOutputs,
      }),
    );
    wantCenterRef.current = !hasUserLayout;
    if (!hasUserLayout) {
      setCenterNonce((n) => n + 1);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [structureKey, workflowPath]);

  // After real node heights are known, re-pack columns so stacked jobs keep a
  // visible gap (estimates alone were shorter than the DOM and cards looked glued).
  useEffect(() => {
    if (isolateSnapRef.current != null) return;
    if (!nodesInitialized || nodes.length === 0) return;
    if (skipPackRef.current) return;
    if (packedEpochRef.current === layoutEpochRef.current) return;

    const measured = getNodes();
    const ready =
      measured.length > 0 &&
      measured.every(
        (n) => (n.measured?.width ?? 0) > 0 && (n.measured?.height ?? 0) > 0,
      );
    if (!ready) return;

    const heights = new Map<string, number>();
    const environments = new Map<string, string | null>();
    for (const n of measured) {
      heights.set(n.id, n.measured?.height ?? 0);
      const env = (n.data as JobNodeData | undefined)?.environmentName ?? null;
      environments.set(n.id, env);
    }
    const packed = packStackedGaps(
      positionsRef.current,
      heights,
      GAP_Y,
      environments,
    );
    if (graph) {
      const boxes = measured.map((n) => {
        const pos = packed.get(n.id) ?? n.position;
        const env =
          (n.data as JobNodeData | undefined)?.environmentName ?? "";
        return {
          id: n.id,
          env,
          x: pos.x,
          y: pos.y,
          width: n.measured?.width ?? NODE_WIDTH,
          height: n.measured?.height ?? heights.get(n.id) ?? 0,
        };
      });
      applyBandEviction(packed, boxes, graph.edges);
    }
    let moved = false;
    for (const [id, pos] of packed) {
      const prev = positionsRef.current.get(id);
      if (!prev || Math.abs(prev.y - pos.y) > 0.5 || Math.abs(prev.x - pos.x) > 0.5) {
        moved = true;
        break;
      }
    }
    packedEpochRef.current = layoutEpochRef.current;
    if (!moved) return;

    positionsRef.current = packed;
    setNodes((prev) =>
      prev.map((n) => ({
        ...n,
        position: packed.get(n.id) ?? n.position,
      })),
    );
    wantCenterRef.current = true;
    setCenterNonce((n) => n + 1);
  }, [nodesInitialized, nodes.length, getNodes, setNodes, structureKey, graph]);

  // Center after nodes are measured. First paint often runs before the flex
  // pane has its final size (top-left bias); refit when the container grows and
  // always do a second fitView on the next frame.
  useEffect(() => {
    if (isolateSnapRef.current != null) return;
    if (!wantCenterRef.current || !nodesInitialized || nodes.length === 0) {
      return;
    }
    let cancelled = false;
    let retries = 0;

    const doFit = () =>
      fitView({
        padding: nodes.length <= 2 ? 0.4 : nodes.length <= 6 ? 0.28 : 0.2,
        maxZoom: 1.15,
        minZoom: 0.12,
        duration: 0,
      });

    const attempt = () => {
      if (cancelled || !wantCenterRef.current || isolateSnapRef.current != null) {
        return;
      }
      const measured = getNodes();
      const ready =
        measured.length > 0 &&
        measured.every(
          (n) => (n.measured?.width ?? 0) > 0 && (n.measured?.height ?? 0) > 0,
        );
      if (!ready && retries < 16) {
        retries += 1;
        requestAnimationFrame(attempt);
        return;
      }
      void doFit().then(() => {
        if (cancelled) return;
        // Second pass after React Flow applies the first transform.
        requestAnimationFrame(() => {
          if (cancelled || !wantCenterRef.current || isolateSnapRef.current != null) {
            return;
          }
          void doFit().then(() => {
            if (!cancelled && isolateSnapRef.current == null) {
              wantCenterRef.current = false;
            }
          });
        });
      });
    };

    let inner = 0;
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(attempt);
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(outer);
      cancelAnimationFrame(inner);
    };
  }, [nodesInitialized, nodes.length, fitView, getNodes, centerNonce]);

  // On first load the center panel often resizes after the initial fitView
  // (sidebars/fonts). Refit once the pane has a stable non-zero size.
  useEffect(() => {
    const el = flowRootRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width < 32 || height < 32) return;
      const prev = paneSizeRef.current;
      const grew =
        prev.w === 0 ||
        Math.abs(width - prev.w) > 24 ||
        Math.abs(height - prev.h) > 24;
      paneSizeRef.current = { w: width, h: height };
      if (!grew) return;
      if (!wantCenterRef.current && prev.w === 0) {
        // First meaningful size after mount — allow one more center pass.
        wantCenterRef.current = true;
      }
      if (wantCenterRef.current) {
        setCenterNonce((n) => n + 1);
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Patch node data (selection, run states, edge focus) without moving nodes.
  useEffect(() => {
    if (!graph) return;
    setNodes((prev) =>
      prev.map((node) => {
        const inst = instances.find((i) => i.instanceId === node.id);
        if (!inst) return node;
        const isolated = isolateSnapRef.current != null;
        const isolateHidden =
          isolated && edgeFocus != null && !edgeFocus.nodes.has(node.id);
        const runStateHidden =
          runVisibleIds != null && !runVisibleIds.has(node.id);
        const hidden = isolateHidden || runStateHidden;
        const pos = isolated
          ? node.position
          : (positionsRef.current.get(node.id) ?? node.position);
        const data = buildNodeData(
          view,
          workflowPath,
          workflowIndex,
          selection,
          inst,
          onSelectJob,
          onSelectStep,
          edgeFocus,
        );
        return {
          ...node,
          hidden,
          position: pos,
          data,
          draggable: !data.dimmed && !hidden,
        };
      }),
    );
  }, [
    view,
    selection,
    instances,
    graph,
    workflowPath,
    workflowIndex,
    onSelectJob,
    onSelectStep,
    edgeFocus,
    runVisibleIds,
    setNodes,
  ]);

  // Keep edges in sync when bindings/view/focus change
  useEffect(() => {
    if (!graph) return;
    setEdges(
      buildEdges(
        view,
        graph,
        workflowPath,
        instances,
        edgeFocus,
        edgeColors,
        {
          showNeeds,
          showOutputs,
        },
        visibleIds,
      ),
    );
  }, [
    view,
    graph,
    workflowPath,
    instances,
    edgeFocus,
    edgeColors,
    showNeeds,
    showOutputs,
    visibleIds,
    setEdges,
  ]);

  const captureSnap = useCallback(() => {
    if (isolateSnapRef.current != null) return;
    isolateSnapRef.current = {
      positions: new Map(
        [...positionsRef.current].map(([id, pos]) => [id, { x: pos.x, y: pos.y }]),
      ),
      viewport: getViewport(),
    };
  }, [getViewport]);

  const restoreSnap = useCallback(() => {
    const snap = isolateSnapRef.current;
    if (!snap) return;
    isolateFitGenerationRef.current += 1;
    isolateFitIdsRef.current = null;
    setIsolateFitNonce((n) => n + 1);
    const positions = new Map(
      [...snap.positions].map(([id, pos]) => [id, { x: pos.x, y: pos.y }]),
    );
    positionsRef.current = positions;
    SESSION_LAYOUTS.set(workflowPath, new Map(positions));
    setViewport(snap.viewport);
    isolateSnapRef.current = null;
    setNodes((prev) =>
      prev.map((node) => {
        const hidden = runVisibleIds != null && !runVisibleIds.has(node.id);
        return {
          ...node,
          hidden,
          position: positions.get(node.id) ?? node.position,
          draggable:
            !hidden && !(edgeFocus != null && !edgeFocus.nodes.has(node.id)),
        };
      }),
    );
  }, [edgeFocus, runVisibleIds, setNodes, setViewport, workflowPath]);

  const applyIsolate = useCallback(
    (focus: EdgeFocus) => {
      if (!graph) return;
      captureSnap();
      const visible = focus.nodes;
      const visibleIds = graph.nodes.filter((id) => visible.has(id));
      const heights = new Map<string, number>();
      const environments = new Map<string, string | null>();
      for (const inst of instances) {
        if (!visible.has(inst.instanceId)) continue;
        heights.set(inst.instanceId, estimateNodeHeight(inst.steps.length));
        environments.set(inst.instanceId, inst.environment?.name ?? null);
      }
      const laid = layoutIsolatedNodes(
        visibleIds,
        graph.edges,
        heights,
        environments,
        densityEdgePairs(graph, view, workflowPath, instances),
      );
      const base =
        isolateSnapRef.current?.positions ?? positionsRef.current;
      setNodes((prev) =>
        withIsolateHidden(prev, visible).map((node) => {
          const inst = instances.find((i) => i.instanceId === node.id);
          const data = inst
            ? buildNodeData(
                view,
                workflowPath,
                workflowIndex,
                selection,
                inst,
                onSelectJob,
                onSelectStep,
                focus,
              )
            : node.data;
          return {
            ...node,
            position: laid.get(node.id) ?? base.get(node.id) ?? node.position,
            data,
            draggable: !data.dimmed && !node.hidden,
          };
        }),
      );
      isolateFitIdsRef.current = visibleIds;
      isolateFitGenerationRef.current += 1;
      setIsolateFitNonce((n) => n + 1);
    },
    [
      captureSnap,
      graph,
      instances,
      onSelectJob,
      onSelectStep,
      selection,
      setNodes,
      view,
      workflowIndex,
      workflowPath,
    ],
  );

  useEffect(() => {
    const ids = isolateFitIdsRef.current;
    if (!ids || ids.length === 0) return;
    const generation = isolateFitGenerationRef.current;
    let cancelled = false;
    const padding = ids.length <= 2 ? 0.4 : ids.length <= 6 ? 0.28 : 0.2;
    const run = () => {
      if (
        cancelled ||
        generation !== isolateFitGenerationRef.current ||
        isolateFitIdsRef.current == null
      ) return;
      void fitView({
        nodes: ids.map((id) => ({ id })),
        padding,
        maxZoom: 1.15,
        minZoom: 0.12,
        duration: 0,
      });
    };
    const outer = requestAnimationFrame(() => {
      requestAnimationFrame(run);
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(outer);
    };
  }, [isolateFitNonce, fitView]);

  const onNodeDragStop = useCallback(
    (_: MouseEvent | TouchEvent, node: Node<JobNodeData>) => {
      if (node.hidden || node.data.dimmed) return;
      if (isolateSnapRef.current != null) return;
      if (!graph) {
        positionsRef.current.set(node.id, { ...node.position });
        SESSION_LAYOUTS.set(workflowPath, new Map(positionsRef.current));
        return;
      }
      for (const n of nodes) {
        positionsRef.current.set(
          n.id,
          n.id === node.id ? { ...node.position } : { ...n.position },
        );
      }
      const heightFor = (id: string) => {
        const inst = instances.find((i) => i.instanceId === id);
        return estimateNodeHeight(inst?.steps.length ?? 3);
      };
      const boxes = boxesFromNodes(
        nodes.map((n) =>
          n.id === node.id
            ? { ...n, position: { ...node.position } }
            : n,
        ),
        heightFor,
        { envOnly: false },
      );
      applyBandEviction(positionsRef.current, boxes, graph.edges);
      const settledBoxes = boxesFromNodes(
        nodes.map((n) => ({
          ...n,
          position:
            positionsRef.current.get(n.id) ??
            (n.id === node.id ? node.position : n.position),
        })),
        heightFor,
        { envOnly: false },
      );
      const dropped = settledBoxes.find((box) => box.id === node.id);
      if (dropped) {
        const toNudgeBox = (box: typeof dropped) => ({
          id: box.id,
          x: box.x,
          y: box.y,
          w: box.width,
          h: box.height,
        });
        const nudged = nudgeDroppedBox(
          toNudgeBox(dropped),
          settledBoxes
            .filter((box) => box.id !== node.id)
            .map(toNudgeBox),
        );
        positionsRef.current.set(node.id, nudged);
      }
      SESSION_LAYOUTS.set(workflowPath, new Map(positionsRef.current));
      setNodes((prev) =>
        prev.map((n) => ({
          ...n,
          position: positionsRef.current.get(n.id) ?? n.position,
        })),
      );
    },
    [graph, nodes, instances, workflowPath, setNodes],
  );

  const onNodeClick = useCallback(
    (_: ReactMouseEvent, node: Node<JobNodeData>) => {
      if (node.hidden || node.data.dimmed) return;
      setEdgeFocus(null);
      restoreSnap();
      onSelectJob(workflowIndex, node.id, workflowPath);
    },
    [onSelectJob, restoreSnap, workflowIndex, workflowPath],
  );

  const onEdgeClick = useCallback(
    (event: ReactMouseEvent, edge: Edge<FlowEdgeData>) => {
      event.stopPropagation();
      if (!graph) return;
      const focus =
        edge.data?.kind === "dataflow"
          ? focusForDataflowEdge(view, workflowPath, instances, edge)
          : focusForNeedsEdge(graph.edges, edge);
      if (isolateMode) applyIsolate(focus);
      setEdgeFocus(focus);
    },
    [applyIsolate, graph, instances, isolateMode, view, workflowPath],
  );

  const onPaneClick = useCallback(() => {
    setEdgeFocus(null);
    restoreSnap();
  }, [restoreSnap]);

  if (!graph || graph.nodes.length === 0) {
    return (
      <div className="empty-state">
        <p>No graph nodes for this workflow</p>
      </div>
    );
  }

  return (
    <div className="graph-flow-root" ref={flowRootRef}>
    <ReactFlow
      nodes={nodes}
      edges={edges}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onNodeClick={onNodeClick}
      onEdgeClick={onEdgeClick}
      onPaneClick={onPaneClick}
      onNodeDragStop={onNodeDragStop}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
      minZoom={0.15}
      maxZoom={1.5}
      proOptions={{ hideAttribution: true }}
      nodesDraggable
      nodesConnectable={false}
      elementsSelectable
      onlyRenderVisibleElements
      onInit={() => {
        // Pane is ready; re-run center if layout still wants it (first paint /
        // StrictMode often fits before the flex panel has its final size).
        if (wantCenterRef.current) {
          setCenterNonce((n) => n + 1);
        }
      }}
    >
      <EnvBandsOverlay
        nodes={nodes}
        instances={instances}
        needsEdges={graph.edges}
        ready={nodesInitialized}
      />
      <Background color="var(--border)" gap={20} />
      <Controls className="flow-controls" />
      <Panel position="top-left" className="flow-toolbar">
        {ALL_RUN_STATES.map((state) => (
          <button
            key={state}
            type="button"
            aria-pressed={runStateFilters.has(state)}
            title={RUN_STATE_LABELS[state]}
            onClick={() => onToggleRunStateFilter(state)}
          >
            {RUN_STATE_LABELS[state]}
          </button>
        ))}
        <button
          type="button"
          aria-pressed={showNeeds}
          title="Show depends-on lines"
          onClick={() => setShowNeeds((shown) => !shown)}
        >
          Depends
        </button>
        <button
          type="button"
          aria-pressed={showOutputs}
          title="Show output lines"
          onClick={() => setShowOutputs((shown) => !shown)}
        >
          Outputs
        </button>
        <button
          type="button"
          aria-pressed={isolateMode}
          title="Hide unrelated jobs on line click"
          onClick={() => {
            if (isolateMode) {
              if (edgeFocus) restoreSnap();
              setIsolateMode(false);
            } else {
              setIsolateMode(true);
              if (edgeFocus) applyIsolate(edgeFocus);
            }
          }}
        >
          Isolate
        </button>
      </Panel>
      <Panel position="bottom-center" className="flow-legend">
        <span className="flow-legend__item">
          <span className="flow-legend__line flow-legend__line--needs" />
          depends on
        </span>
        <span className="flow-legend__item">
          <span className="flow-legend__line flow-legend__line--dataflow" />
          passes output
        </span>
      </Panel>
      <MiniMap
        className="flow-minimap"
        nodeColor={needsColor}
        maskColor={`color-mix(in srgb, ${minimapMask} 78%, transparent)`}
        pannable
        zoomable
      />
    </ReactFlow>
    </div>
  );
}

export default function GraphPane(props: GraphPaneProps) {
  const wf = props.view.workflows.find((w) => w.index === props.workflowIndex);
  const label = wf?.name ?? wf?.path ?? "Workflow";

  return (
    <div className="graph-pane">
      <div className="graph-pane__workflow-name">{label}</div>
      <ReactFlowProvider>
        {/* Remount provider canvas state when switching workflows */}
        <GraphCanvas key={`${props.workflowIndex}:${wf?.path ?? ""}`} {...props} />
      </ReactFlowProvider>
    </div>
  );
}
