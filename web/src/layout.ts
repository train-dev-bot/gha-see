const NODE_WIDTH = 320;
/** Header + meta chrome (padding, title, runs-on, depends line). */
const NODE_HEIGHT_BASE = 130;
/** Collapsed step row incl. chips + step gap — must not under-estimate real DOM. */
const STEP_ROW = 102;
/** Compact horizontal gutter — readable at fitView without a huge canvas. */
const GAP_X = 180;
/** Mild extra gap per unique job-pair that spans a column gutter. */
const GAP_X_PER_EDGE = 14;
/** Cap on density-driven extra gap (keep showcases viewable). */
const GAP_X_EXTRA_CAP = 120;
/** Crossing pairs allowed before the column gap starts growing. */
const GAP_X_BASE_CROSS = 4;
/** Minimum vertical gutter between stacked job cards in one column. */
const GAP_Y = 64;
/**
 * Extra Y between different-env cards so band chrome (padding 18×2 + label 28)
 * plus a border gap equal to GAP_Y fits — keep in sync with envBands.ts.
 */
const ENV_BAND_CHROME_Y = 18 + 18 + 28;
/**
 * Soft vertical stagger between hub successors (center-to-center), not full
 * card stacking — enough to unbundle edges without blowing up the canvas.
 */
const HUB_STAGGER = 56;
/**
 * Per-column vertical wave amplitude. Alternating columns shift up/down so the
 * pipeline is not a straight midline and long edges separate visually.
 */
const COLUMN_WAVE_AMP = 110;

export type NudgeBox = {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
};

/** Whether two boxes violate the required empty space on both axes. */
export function boxesOverlap(a: NudgeBox, b: NudgeBox, gap: number): boolean {
  return (
    a.x < b.x + b.w + gap &&
    a.x + a.w + gap > b.x &&
    a.y < b.y + b.h + gap &&
    a.y + a.h + gap > b.y
  );
}

/**
 * Move only the dropped box to the nearest side of each blocker. If a bounded
 * search cannot find a clear position, preserve the user's original drop.
 */
export function nudgeDroppedBox(
  dropped: NudgeBox,
  others: NudgeBox[],
  gap: number = GAP_Y,
  maxRounds: number = 24,
): { x: number; y: number } {
  const original = { x: dropped.x, y: dropped.y };
  let current = original;

  for (let round = 0; round < maxRounds; round++) {
    const placed = { ...dropped, ...current };
    const hit = others.find((other) => boxesOverlap(placed, other, gap));
    if (!hit) return current;

    const candidates = [
      { x: current.x, y: hit.y - dropped.h - gap },
      { x: current.x, y: hit.y + hit.h + gap },
      { x: hit.x - dropped.w - gap, y: current.y },
      { x: hit.x + hit.w + gap, y: current.y },
    ]
      .filter(
        (candidate) =>
          !boxesOverlap({ ...dropped, ...candidate }, hit, gap),
      )
      .sort(
        (a, b) =>
          Math.hypot(a.x - original.x, a.y - original.y) -
          Math.hypot(b.x - original.x, b.y - original.y),
      );

    if (candidates.length === 0) return original;
    current = candidates[0]!;
  }

  const placed = { ...dropped, ...current };
  return others.some((other) => boxesOverlap(placed, other, gap))
    ? original
    : current;
}

/** 0, +A, 0, -A, … for consecutive DAG columns. */
export function columnWaveOffset(columnIndex: number): number {
  return Math.sin((columnIndex * Math.PI) / 2) * COLUMN_WAVE_AMP;
}

/** Horizontal gap after a DAG column given how many edges cross that gap. */
export function gapAfterCrossCount(crossCount: number): number {
  const extra = Math.min(
    GAP_X_EXTRA_CAP,
    Math.max(0, crossCount - GAP_X_BASE_CROSS) * GAP_X_PER_EDGE,
  );
  return GAP_X + extra;
}

/**
 * Edges that visually travel through the gutter after `level`
 * (source at or before this column, target at or after the next).
 * Counts unique from→to pairs so dataflow bindings don't explode gaps.
 */
function countSpanCrossings(
  level: number,
  levels: Map<string, number>,
  pairs: [string, string][],
): number {
  const seen = new Set<string>();
  for (const [from, to] of pairs) {
    const a = levels.get(from) ?? 0;
    const b = levels.get(to) ?? 0;
    if (a <= level && b >= level + 1) {
      seen.add(`${from}\0${to}`);
    }
  }
  return seen.size;
}

export function estimateNodeHeight(stepCount: number): number {
  const visible = Math.min(Math.max(stepCount, 1), 6);
  return NODE_HEIGHT_BASE + visible * STEP_ROW;
}

export function computeLevels(
  nodes: string[],
  edges: [string, string][],
): Map<string, number> {
  const inDegree = new Map<string, number>();
  const adj = new Map<string, string[]>();

  for (const n of nodes) {
    inDegree.set(n, 0);
    adj.set(n, []);
  }

  for (const [from, to] of edges) {
    if (!adj.has(from)) adj.set(from, []);
    if (!inDegree.has(to)) inDegree.set(to, 0);
    adj.get(from)!.push(to);
    inDegree.set(to, (inDegree.get(to) ?? 0) + 1);
  }

  const levels = new Map<string, number>();
  const queue: string[] = [];

  for (const n of nodes) {
    if ((inDegree.get(n) ?? 0) === 0) {
      queue.push(n);
      levels.set(n, 0);
    }
  }

  if (queue.length === 0 && nodes.length > 0) {
    queue.push(nodes[0]);
    levels.set(nodes[0], 0);
  }

  const visited = new Set<string>();
  while (queue.length > 0) {
    const n = queue.shift()!;
    if (visited.has(n)) continue;
    visited.add(n);
    const level = levels.get(n) ?? 0;

    for (const child of adj.get(n) ?? []) {
      const next = level + 1;
      const prev = levels.get(child);
      if (prev === undefined || next > prev) {
        levels.set(child, next);
      }
      inDegree.set(child, (inDegree.get(child) ?? 1) - 1);
      if ((inDegree.get(child) ?? 0) <= 0) {
        queue.push(child);
      }
    }
  }

  for (const n of nodes) {
    if (!levels.has(n)) levels.set(n, 0);
  }

  return levels;
}

function buildPredMap(edges: [string, string][]): Map<string, string[]> {
  const preds = new Map<string, string[]>();
  for (const [from, to] of edges) {
    if (!preds.has(to)) preds.set(to, []);
    preds.get(to)!.push(from);
  }
  return preds;
}

/** Average center-Y of already-placed predecessors (Sugiyama barycenter). */
function barycenterY(
  id: string,
  preds: Map<string, string[]>,
  positions: Map<string, { x: number; y: number }>,
  heights: Map<string, number> | undefined,
): number | null {
  const parents = preds.get(id) ?? [];
  let sum = 0;
  let count = 0;
  for (const p of parents) {
    const pos = positions.get(p);
    if (!pos) continue;
    const h = heights?.get(p) ?? estimateNodeHeight(3);
    sum += pos.y + h / 2;
    count += 1;
  }
  if (count === 0) return null;
  return sum / count;
}

/**
 * Sort column ids: env clusters first (for bands), then barycenter within
 * each env so edges between adjacent columns cross less.
 */
function orderColumn(
  ids: string[],
  environments: Map<string, string | null | undefined> | undefined,
  preds: Map<string, string[]>,
  positions: Map<string, { x: number; y: number }>,
  heights: Map<string, number> | undefined,
): string[] {
  const score = (id: string) => {
    const bc = barycenterY(id, preds, positions, heights);
    return bc ?? Number.POSITIVE_INFINITY;
  };

  return [...ids].sort((a, b) => {
    const ea = (environments?.get(a) ?? "").trim();
    const eb = (environments?.get(b) ?? "").trim();
    if (!ea && eb) return 1;
    if (ea && !eb) return -1;
    const byEnv = ea.localeCompare(eb);
    if (byEnv !== 0) return byEnv;
    const sa = score(a);
    const sb = score(b);
    if (sa !== sb) return sa - sb;
    return a.localeCompare(b);
  });
}

/** Topological column layout; stacks nodes in a column using per-node heights.
 * When `environments` is provided, jobs with the same env name are stacked
 * together within each DAG column (approach C layout bias).
 * `densityEdges` (needs + dataflow) widens gutters for every edge that spans
 * a column gap (including long-range needs like build→promote).
 */
export function layoutNodes(
  nodes: string[],
  edges: [string, string][],
  heights?: Map<string, number>,
  environments?: Map<string, string | null | undefined>,
  densityEdges?: [string, string][],
): Map<string, { x: number; y: number }> {
  const levels = computeLevels(nodes, edges);
  const byLevel = new Map<number, string[]>();

  for (const n of nodes) {
    const level = levels.get(n) ?? 0;
    if (!byLevel.has(level)) byLevel.set(level, []);
    byLevel.get(level)!.push(n);
  }

  const pairs = densityEdges ?? edges;
  const preds = buildPredMap(pairs);
  const sortedLevels = [...byLevel.keys()].sort((a, b) => a - b);
  const levelX = new Map<number, number>();
  let cursorX = 0;
  for (let i = 0; i < sortedLevels.length; i++) {
    const level = sortedLevels[i]!;
    levelX.set(level, cursorX);
    if (i < sortedLevels.length - 1) {
      const cross = countSpanCrossings(level, levels, pairs);
      cursorX += NODE_WIDTH + gapAfterCrossCount(cross);
    }
  }

  const positions = new Map<string, { x: number; y: number }>();

  for (const level of sortedLevels) {
    const ids = orderColumn(
      byLevel.get(level)!,
      environments,
      preds,
      positions,
      heights,
    );
    const x = levelX.get(level) ?? level * (NODE_WIDTH + GAP_X);
    const placed: Array<{ id: string; y: number; h: number }> = [];

    for (const id of ids) {
      const h = heights?.get(id) ?? estimateNodeHeight(3);
      const bc = barycenterY(id, preds, positions, heights);
      // Prefer barycenter top so the node center tracks predecessors.
      let placeY = bc != null ? bc - h / 2 : 0;
      if (placeY < 0) placeY = 0;

      let guard = 0;
      while (guard++ < 64) {
        const overlap = placed.some((p) => {
          const gap = gapYBetween(environments, p.id, id, GAP_Y);
          return placeY < p.y + p.h + gap && placeY + h + gap > p.y;
        });
        if (!overlap) break;
        // Push below the conflicting card (stable top-to-bottom packing).
        const blocker = placed
          .filter((p) => {
            const gap = gapYBetween(environments, p.id, id, GAP_Y);
            return placeY < p.y + p.h + gap && placeY + h + gap > p.y;
          })
          .reduce((a, b) => (a.y + a.h > b.y + b.h ? a : b));
        placeY =
          blocker.y + blocker.h + gapYBetween(environments, blocker.id, id, GAP_Y);
      }

      positions.set(id, { x, y: placeY });
      placed.push({ id, y: placeY, h });
    }
  }

  // Fan successors of busy hubs into vertical lanes (avoids a single horizontal
  // "cable" of overlapping edges — matches the manual drag preference).
  spreadHubSuccessors(positions, pairs, heights);
  resolveColumnOverlaps(positions, byLevel, heights, environments);

  // Column wave: shift whole columns up/down so chains aren't a straight line.
  for (let i = 0; i < sortedLevels.length; i++) {
    const dy = columnWaveOffset(i);
    if (dy === 0) continue;
    for (const id of byLevel.get(sortedLevels[i]!) ?? []) {
      const p = positions.get(id);
      if (p) positions.set(id, { x: p.x, y: p.y + dy });
    }
  }

  // Pull the whole graph up so fitView doesn't keep empty top margin.
  let minY = Infinity;
  for (const p of positions.values()) minY = Math.min(minY, p.y);
  if (Number.isFinite(minY) && minY !== 0) {
    for (const [id, p] of positions) {
      positions.set(id, { x: p.x, y: p.y - minY });
    }
  }

  return positions;
}

/** Direct out-neighbors, unique, stable order. */
function outNeighbors(edges: [string, string][]): Map<string, string[]> {
  const outs = new Map<string, string[]>();
  for (const [from, to] of edges) {
    if (!outs.has(from)) outs.set(from, []);
    const list = outs.get(from)!;
    if (!list.includes(to)) list.push(to);
  }
  return outs;
}

/**
 * High out-degree jobs otherwise leave successors on one corridor.
 * Nudge successor centers by a small stagger (not full card lanes) so edges
 * fan slightly while the graph stays compact enough to read at fitView.
 */
function spreadHubSuccessors(
  positions: Map<string, { x: number; y: number }>,
  edges: [string, string][],
  heights: Map<string, number> | undefined,
): void {
  const outs = outNeighbors(edges);
  const hubs = [...outs.entries()]
    .filter(([, targets]) => targets.length >= 3)
    .sort(
      (a, b) =>
        (positions.get(a[0])?.x ?? 0) - (positions.get(b[0])?.x ?? 0),
    );

  const claimed = new Set<string>();
  for (const [hub, targets] of hubs) {
    const hubPos = positions.get(hub);
    if (!hubPos) continue;
    const hubH = heights?.get(hub) ?? estimateNodeHeight(3);
    const hubCy = hubPos.y + hubH / 2;

    const unclaimed = targets.filter(
      (t) => t !== hub && positions.has(t) && !claimed.has(t),
    );
    if (unclaimed.length < 2) continue;

    unclaimed.sort((a, b) => {
      const ya = positions.get(a)!.y;
      const yb = positions.get(b)!.y;
      if (ya !== yb) return ya - yb;
      return a.localeCompare(b);
    });

    const mid = (unclaimed.length - 1) / 2;
    for (let i = 0; i < unclaimed.length; i++) {
      const id = unclaimed[i]!;
      const h = heights?.get(id) ?? estimateNodeHeight(3);
      const pos = positions.get(id)!;
      const centerY = hubCy + (i - mid) * HUB_STAGGER;
      positions.set(id, { x: pos.x, y: Math.max(0, centerY - h / 2) });
      claimed.add(id);
    }
  }
}

/** After hub spreading, push overlapping cards apart within each DAG column. */
function resolveColumnOverlaps(
  positions: Map<string, { x: number; y: number }>,
  byLevel: Map<number, string[]>,
  heights: Map<string, number> | undefined,
  environments?: Map<string, string | null | undefined>,
): void {
  for (const ids of byLevel.values()) {
    packColumnIds(positions, ids, heights, GAP_Y, environments);
  }
}

/**
 * Vertical gutter between two stacked jobs. Different environments need room
 * for both env-band borders plus the same min gap used between job cards.
 */
function gapYBetween(
  environments: Map<string, string | null | undefined> | undefined,
  upperId: string,
  lowerId: string,
  minGap: number,
): number {
  if (!environments) return minGap;
  const ea = (environments.get(upperId) ?? "").trim();
  const eb = (environments.get(lowerId) ?? "").trim();
  if (!ea || !eb || ea === eb) return minGap;
  return minGap + ENV_BAND_CHROME_Y;
}

/**
 * Re-pack nodes that share a column (same x or explicit id list) so consecutive
 * cards keep at least `minGap` of empty space — uses real measured heights when
 * available so estimates cannot eat the gutter. Different-env neighbors get
 * extra space so their env-band rectangles do not overlap.
 */
export function packStackedGaps(
  positions: Map<string, { x: number; y: number }>,
  heights: Map<string, number>,
  minGap: number = GAP_Y,
  environments?: Map<string, string | null | undefined>,
): Map<string, { x: number; y: number }> {
  const byX = new Map<number, string[]>();
  for (const [id, p] of positions) {
    if (!byX.has(p.x)) byX.set(p.x, []);
    byX.get(p.x)!.push(id);
  }
  const next = new Map(positions);
  for (const ids of byX.values()) {
    if (ids.length < 2) continue;
    packColumnIds(next, ids, heights, minGap, environments);
  }
  return next;
}

function packColumnIds(
  positions: Map<string, { x: number; y: number }>,
  ids: string[],
  heights: Map<string, number> | undefined,
  minGap: number = GAP_Y,
  environments?: Map<string, string | null | undefined>,
): void {
  const ordered = [...ids].sort(
    (a, b) => (positions.get(a)?.y ?? 0) - (positions.get(b)?.y ?? 0),
  );
  const first = positions.get(ordered[0]!);
  if (!first) return;
  let y = first.y;
  let prevId: string | undefined;
  for (const id of ordered) {
    const pos = positions.get(id);
    if (!pos) continue;
    const h = heights?.get(id) ?? estimateNodeHeight(3);
    if (prevId) y += gapYBetween(environments, prevId, id, minGap);
    positions.set(id, { x: pos.x, y });
    y += h;
    prevId = id;
  }
}

export { NODE_WIDTH, GAP_X, GAP_Y };
