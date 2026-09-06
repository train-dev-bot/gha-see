import { describe, expect, it } from "vitest";
import { preservedActiveIndex } from "./preserveWorkflow";

describe("preservedActiveIndex", () => {
  const workflows = [
    { index: 0, path: "/repo/.github/workflows/a.yml" },
    { index: 1, path: "/repo/.github/workflows/b.yml" },
    { index: 2, path: "/repo/.github/workflows/c.yml" },
  ];

  it("keeps the previously opened workflow by path after a reload", () => {
    expect(
      preservedActiveIndex(workflows, {
        index: 1,
        path: "/repo/.github/workflows/b.yml",
      }),
    ).toBe(1);
  });

  it("follows the same file when reload reassigns indexes", () => {
    const afterInsert = [
      { index: 0, path: "/repo/.github/workflows/0-new.yml" },
      { index: 1, path: "/repo/.github/workflows/a.yml" },
      { index: 2, path: "/repo/.github/workflows/b.yml" },
      { index: 3, path: "/repo/.github/workflows/c.yml" },
    ];
    expect(
      preservedActiveIndex(afterInsert, {
        index: 1,
        path: "/repo/.github/workflows/b.yml",
      }),
    ).toBe(2);
  });

  it("falls back to the first workflow when the file disappeared", () => {
    expect(
      preservedActiveIndex(workflows, {
        index: 9,
        path: "/repo/.github/workflows/gone.yml",
      }),
    ).toBe(0);
  });
});
