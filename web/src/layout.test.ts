import { describe, expect, it } from "vitest";
import { NODE_WIDTH, GAP_Y, boxesOverlap, nudgeDroppedBox } from "./layout";

const a = { id: "a", x: 0, y: 0, w: NODE_WIDTH, h: 100 };
const b = { id: "b", x: 0, y: 0, w: NODE_WIDTH, h: 100 };

describe("nudgeDroppedBox", () => {
  it("leaves a non-overlapping drop in place", () => {
    const far = { ...b, id: "b", y: 400 };
    expect(nudgeDroppedBox(a, [far])).toEqual({ x: 0, y: 0 });
  });

  it("moves the dropped box off a stack, not the other", () => {
    const next = nudgeDroppedBox(a, [b]);
    expect(next).not.toEqual({ x: 0, y: 0 });
    expect(boxesOverlap({ ...a, ...next }, b, GAP_Y)).toBe(false);
  });
});
