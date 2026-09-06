import { NODE_WIDTH, GAP_Y } from "./layout";

export type EnvJobBox = {
  id: string;
  env: string;
  x: number;
  y: number;
  width: number;
  height: number;
};

export type JobBox = {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
};

export type EnvBand = {
  id: string;
  env: string;
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
  memberIds: string[];
};

/** Side/bottom padding around member AABB. */
export const ENV_BAND_PADDING = 18;
/** Extra top space so the env label is not covered by job cards. */
export const ENV_BAND_LABEL_GUTTER = 28;
/** Gap when pushing unrelated jobs out of a band. */
export const ENV_BAND_EVICT_GAP = 48;
/** Minimum empty space between two env-band borders (same idea as GAP_Y for cards). */
export const ENV_BAND_MIN_GAP = GAP_Y;

function hashEnv(name: string): number {
  let h = 0;
  for (let i = 0; i < name.length; i++) {
    h = (h * 31 + name.charCodeAt(i)) >>> 0;
  }
  return h;
}

/** Stable vivid color for an environment name (CSS color string). */
export function envColor(name: string): string {
  const hue = hashEnv(name) % 360;
  return `hsl(${hue} 58% 52%)`;
}

function aabbBand(
  env: string,
  members: EnvJobBox[],
  clusterIdx: number,
  bandPadding: number,
  labelGutter: number,
): EnvBand {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const m of members) {
    minX = Math.min(minX, m.x);
    minY = Math.min(minY, m.y);
    maxX = Math.max(maxX, m.x + m.width);
    maxY = Math.max(maxY, m.y + m.height);
  }
  return {
    id: `env-band-${env}-${clusterIdx}`,
    env,
    x: minX - bandPadding,
    y: minY - bandPadding - labelGutter,
    width: maxX - minX + bandPadding * 2,
    height: maxY - minY + bandPadding * 2 + labelGutter,
    color: envColor(env),
    memberIds: members.map((m) => m.id),
  };
}

function intersects(
  box: JobBox,
  band: Pick<EnvBand, "x" | "y" | "width" | "height">,
): boolean {
  return (
    box.x < band.x + band.width &&
    box.x + box.width > band.x &&
    box.y < band.y + band.height &&
    box.y + box.height > band.y
  );
}

function xOverlap(
  a: Pick<EnvBand, "x" | "width">,
  b: Pick<EnvBand, "x" | "width">,
): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x;
}

/**
 * Cluster jobs that share an environment when they are related in the needs
 * DAG: a direct same-env edge, or sibling/co-dependent link (shared
 * predecessor or successor). That way CI/CD pairs like promote+rollback
 * (both production, no edge between them) still get a band.
 */
export function clusterEnvBands(
  jobs: EnvJobBox[],
  needsEdges: [string, string][] = [],
  bandPadding: number = ENV_BAND_PADDING,
  labelGutter: number = ENV_BAND_LABEL_GUTTER,
): EnvBand[] {
  const byEnv = new Map<string, EnvJobBox[]>();
  for (const job of jobs) {
    const env = job.env.trim();
    if (!env) continue;
    if (!byEnv.has(env)) byEnv.set(env, []);
    byEnv.get(env)!.push(job);
  }

  const preds = new Map<string, Set<string>>();
  const succs = new Map<string, Set<string>>();
  const touch = (map: Map<string, Set<string>>, key: string, value: string) => {
    if (!map.has(key)) map.set(key, new Set());
    map.get(key)!.add(value);
  };
  for (const [from, to] of needsEdges) {
    touch(succs, from, to);
    touch(preds, to, from);
  }
  const shares = (a: string, b: string, map: Map<string, Set<string>>) => {
    const left = map.get(a);
    const right = map.get(b);
    if (!left || !right || left.size === 0 || right.size === 0) return false;
    for (const x of left) {
      if (right.has(x)) return true;
    }
    return false;
  };

  const bands: EnvBand[] = [];

  for (const [env, group] of byEnv) {
    if (group.length === 0) continue;

    // Lone env job: still draw a band around it.
    if (group.length === 1) {
      bands.push(aabbBand(env, group, 0, bandPadding, labelGutter));
      continue;
    }

    const indexOf = new Map(group.map((job, i) => [job.id, i]));
    const parent = group.map((_, i) => i);
    const find = (i: number): number => {
      while (parent[i] !== i) {
        parent[i] = parent[parent[i]];
        i = parent[i];
      }
      return i;
    };
    const union = (a: number, b: number) => {
      const ra = find(a);
      const rb = find(b);
      if (ra !== rb) parent[rb] = ra;
    };

    // Direct needs edge between two same-env jobs.
    for (const [from, to] of needsEdges) {
      const ia = indexOf.get(from);
      const ib = indexOf.get(to);
      if (ia == null || ib == null) continue;
      union(ia, ib);
    }

    // Siblings / co-dependents: share a predecessor or successor in the DAG.
    for (let i = 0; i < group.length; i++) {
      for (let j = i + 1; j < group.length; j++) {
        const a = group[i].id;
        const b = group[j].id;
        if (shares(a, b, preds) || shares(a, b, succs)) {
          union(i, j);
        }
      }
    }

    const clusters = new Map<number, EnvJobBox[]>();
    for (let i = 0; i < group.length; i++) {
      const root = find(i);
      if (!clusters.has(root)) clusters.set(root, []);
      clusters.get(root)!.push(group[i]);
    }

    let clusterIdx = 0;
    for (const members of clusters.values()) {
      if (members.length < 1) continue;
      bands.push(
        aabbBand(env, members, clusterIdx++, bandPadding, labelGutter),
      );
    }
  }

  return bands;
}

/**
 * Push jobs that are not band members but sit inside a band AABB outside
 * the rectangle so unrelated env cards are not visually trapped.
 */
export function evictForeignFromBands(
  boxes: JobBox[],
  bands: EnvBand[],
  gap: number = ENV_BAND_EVICT_GAP,
): Map<string, { x: number; y: number }> {
  const positions = new Map(
    boxes.map((b) => [b.id, { x: b.x, y: b.y }] as const),
  );
  const sizes = new Map(
    boxes.map((b) => [b.id, { width: b.width, height: b.height }] as const),
  );

  for (let pass = 0; pass < 6; pass++) {
    let moved = false;
    for (const band of bands) {
      const members = new Set(band.memberIds);
      for (const box of boxes) {
        if (members.has(box.id)) continue;
        const pos = positions.get(box.id)!;
        const size = sizes.get(box.id)!;
        const live: JobBox = {
          id: box.id,
          x: pos.x,
          y: pos.y,
          width: size.width,
          height: size.height,
        };
        if (!intersects(live, band)) continue;

        // Prefer above the band; if that still collides with another band, go below.
        let nextY = band.y - size.height - gap;
        const trialAbove = { ...live, y: nextY };
        const hitsAbove = bands.some(
          (b) =>
            b.id !== band.id &&
            !new Set(b.memberIds).has(box.id) &&
            intersects(trialAbove, b),
        );
        if (hitsAbove || nextY < -2000) {
          nextY = band.y + band.height + gap;
        }
        positions.set(box.id, { x: pos.x, y: nextY });
        moved = true;
      }
    }
    if (!moved) break;

    // Refresh band AABBs from member positions after eviction? Members didn't move.
    // Foreign moves only — bands stay. Re-check intersections next pass.
  }

  return positions;
}

/**
 * Keep env-band rectangles at least `minGap` apart when they share an X range,
 * by shifting the lower band's members (and later jobs in those columns).
 */
export function packEnvBandGaps(
  positions: Map<string, { x: number; y: number }>,
  boxes: EnvJobBox[],
  needsEdges: [string, string][],
  minGap: number = ENV_BAND_MIN_GAP,
): Map<string, { x: number; y: number }> {
  for (let pass = 0; pass < 8; pass++) {
    const withPos = boxes.map((b) => {
      const p = positions.get(b.id);
      return p ? { ...b, x: p.x, y: p.y } : b;
    });
    const envJobs = withPos.filter((b) => b.env.trim());
    const bands = clusterEnvBands(envJobs, needsEdges);
    if (bands.length < 2) break;

    const ordered = [...bands].sort(
      (a, b) => a.y - b.y || a.x - b.x || a.id.localeCompare(b.id),
    );
    let shifted = false;
    outer: for (let i = 0; i < ordered.length; i++) {
      const upper = ordered[i]!;
      for (let j = i + 1; j < ordered.length; j++) {
        const lower = ordered[j]!;
        if (!xOverlap(upper, lower)) continue;
        const minY = upper.y + upper.height + minGap;
        const dy = minY - lower.y;
        if (dy <= 0.5) continue;
        shiftBandAndFollowers(positions, withPos, lower, dy);
        shifted = true;
        break outer;
      }
    }

    if (!shifted) break;
  }
  return positions;
}

function shiftBandAndFollowers(
  positions: Map<string, { x: number; y: number }>,
  boxes: EnvJobBox[],
  band: EnvBand,
  dy: number,
): void {
  const members = new Set(band.memberIds);
  const thresholdByX = new Map<number, number>();
  for (const id of band.memberIds) {
    const pos = positions.get(id);
    if (!pos) continue;
    const prev = thresholdByX.get(pos.x);
    thresholdByX.set(pos.x, prev == null ? pos.y : Math.min(prev, pos.y));
  }

  for (const box of boxes) {
    const pos = positions.get(box.id);
    if (!pos) continue;
    if (members.has(box.id)) {
      positions.set(box.id, { x: pos.x, y: pos.y + dy });
      continue;
    }
    const thresh = thresholdByX.get(pos.x);
    if (thresh != null && pos.y >= thresh - 0.5) {
      positions.set(box.id, { x: pos.x, y: pos.y + dy });
    }
  }
}

export function boxesFromNodes(
  nodes: Array<{
    id: string;
    position: { x: number; y: number };
    measured?: { width?: number; height?: number };
    data?: { environmentName?: string | null };
  }>,
  fallbackHeight: (id: string) => number = () => 280,
  opts?: { envOnly?: boolean },
): EnvJobBox[] {
  const envOnly = opts?.envOnly !== false;
  const out: EnvJobBox[] = [];
  for (const n of nodes) {
    const env = n.data?.environmentName ?? "";
    if (envOnly && !env) continue;
    out.push({
      id: n.id,
      env: env || "",
      x: n.position.x,
      y: n.position.y,
      width: n.measured?.width ?? NODE_WIDTH,
      height: n.measured?.height ?? fallbackHeight(n.id),
    });
  }
  return out;
}

/** Apply band eviction to a position map (mutates and returns it). */
export function applyBandEviction(
  positions: Map<string, { x: number; y: number }>,
  boxes: EnvJobBox[],
  needsEdges: [string, string][],
): Map<string, { x: number; y: number }> {
  const withPos = boxes.map((b) => {
    const p = positions.get(b.id);
    return p ? { ...b, x: p.x, y: p.y } : b;
  });
  const envJobs = withPos.filter((b) => b.env.trim());
  const bands = clusterEnvBands(envJobs, needsEdges);
  if (bands.length === 0) return positions;
  const evicted = evictForeignFromBands(withPos, bands);
  for (const [id, pos] of evicted) {
    positions.set(id, pos);
  }
  packEnvBandGaps(positions, boxes, needsEdges);
  return positions;
}
