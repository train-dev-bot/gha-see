import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  type Edge,
  type EdgeProps,
} from "@xyflow/react";

export type FlowEdgeKind = "needs" | "dataflow";

export type FlowEdgeData = {
  kind: FlowEdgeKind;
  /** Parallel-slot among edges sharing the same source→target pair. */
  slot: number;
  /** How many edges share this source→target pair. */
  peerCount: number;
  /** Job output name for dataflow edges. */
  outputName?: string;
  /** True when another lineage is focused and this edge is outside it. */
  dimmed?: boolean;
};

export type FlowEdge = Edge<FlowEdgeData, "flow">;

/** Vertical separation between parallel curves (screen px). */
const SLOT_GAP_BASE = 22;
/** Extra px between slots once a pair has more than this many peers. */
const SLOT_GAP_PEER_BOOST = 3;
const SLOT_GAP_PEER_FLOOR = 4;

function offsetForSlot(slot: number, peerCount: number): number {
  if (peerCount <= 1) return 0;
  const gap =
    SLOT_GAP_BASE +
    Math.max(0, peerCount - SLOT_GAP_PEER_FLOOR) * SLOT_GAP_PEER_BOOST;
  return (slot - (peerCount - 1) / 2) * gap;
}

export default function FlowEdgeComponent({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
  label,
  markerEnd,
  style,
  selected,
}: EdgeProps<FlowEdge>) {
  const slot = data?.slot ?? 0;
  const peerCount = data?.peerCount ?? 1;
  const offset = offsetForSlot(slot, peerCount);
  const kind = data?.kind ?? "needs";
  const dimmed = data?.dimmed === true;

  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY: sourceY + offset,
    targetX,
    targetY: targetY + offset,
    sourcePosition,
    targetPosition,
    curvature: kind === "dataflow" ? 0.35 : 0.2,
  });

  const stroke =
    kind === "dataflow" ? "var(--magenta)" : "var(--cyan)";
  const dash = kind === "dataflow" ? "6 4" : undefined;
  const baseWidth = kind === "needs" ? 2.25 : 1.75;
  const strokeWidth = selected ? baseWidth + 1.5 : baseWidth;
  const opacity = dimmed ? 0.22 : 1;

  return (
    <>
      <BaseEdge
        id={id}
        path={edgePath}
        markerEnd={markerEnd}
        interactionWidth={24}
        style={{
          ...style,
          stroke,
          strokeWidth,
          strokeDasharray: dash,
          opacity,
        }}
      />
      {label != null && label !== "" && (
        <EdgeLabelRenderer>
          <div
            className={`edge-label edge-label--${kind}${
              dimmed ? " edge-label--dimmed" : ""
            }${selected ? " edge-label--focused" : ""}`}
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px,${labelY}px)`,
              opacity: dimmed ? 0.22 : 1,
            }}
          >
            {label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}
