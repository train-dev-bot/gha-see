import { useMemo } from "react";
import { ViewportPortal, useStore, type Node } from "@xyflow/react";
import { boxesFromNodes, clusterEnvBands } from "../envBands";
import { estimateNodeHeight } from "../layout";
import type { InstanceDto, JobNodeData } from "../types";

interface EnvBandsOverlayProps {
  /** Controlled job nodes (fallback); live drag positions come from the store. */
  nodes: Node<JobNodeData>[];
  instances: InstanceDto[];
  needsEdges: [string, string][];
  ready: boolean;
}

/** Decorative env regions in flow coordinates — not RF nodes (avoids measure loops). */
export default function EnvBandsOverlay({
  nodes,
  instances,
  needsEdges,
  ready,
}: EnvBandsOverlayProps) {
  // Follow nodes while dragging (store updates every frame; controlled prop may lag).
  const storeNodes = useStore((s) => s.nodes);

  const bands = useMemo(() => {
    if (!ready || nodes.length === 0) return [];
    const heightFor = (id: string) => {
      const inst = instances.find((i) => i.instanceId === id);
      return estimateNodeHeight(inst?.steps.length ?? 3);
    };

    const byId = new Map(nodes.map((n) => [n.id, n]));
    const live = storeNodes.length > 0 ? storeNodes : nodes;
    const merged = live
      .filter((n) => n.type !== "envBand" && byId.has(n.id) && n.hidden !== true)
      .map((n) => {
        const base = byId.get(n.id)!;
        return {
          ...base,
          position: n.position,
          measured: n.measured ?? base.measured,
          data: base.data,
        };
      });

    return clusterEnvBands(boxesFromNodes(merged, heightFor), needsEdges);
  }, [nodes, storeNodes, instances, needsEdges, ready]);

  if (bands.length === 0) return null;

  return (
    <ViewportPortal>
      <div className="env-bands-layer" aria-hidden>
        {bands.map((b) => (
          <div
            key={b.id}
            className="env-band"
            style={{
              transform: `translate(${b.x}px, ${b.y}px)`,
              width: b.width,
              height: b.height,
              borderColor: b.color,
              background: `color-mix(in srgb, ${b.color} 14%, transparent)`,
              boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${b.color} 35%, transparent)`,
            }}
          >
            <span className="env-band__label" style={{ color: b.color }}>
              {b.env}
            </span>
          </div>
        ))}
      </div>
    </ViewportPortal>
  );
}
