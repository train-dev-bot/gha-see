import { describe, expect, it } from "vitest";
import { THEME_STORAGE_KEY } from "./themes";

describe("theme storage key", () => {
  it("is namespaced to gha-see", () => {
    expect(THEME_STORAGE_KEY).toBe("gha-see:theme");
  });
});
