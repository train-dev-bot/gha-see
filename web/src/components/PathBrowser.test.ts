import { describe, expect, it } from "vitest";
import {
  dirnamePath,
  initialBrowsePath,
  joinSave,
  openFolderPath,
  savePathForChoice,
  workflowFileName,
  workflowIndexForPath,
} from "./PathBrowser";

describe("initialBrowsePath", () => {
  it("uses the parent directory of a YAML file path", () => {
    expect(initialBrowsePath("./.github/workflows/scratch.yml")).toBe(
      "./.github/workflows",
    );
    expect(initialBrowsePath("/scratch.yaml")).toBe("/");
    expect(initialBrowsePath("scratch.yml")).toBe(".");
  });

  it("leaves a directory path unchanged", () => {
    expect(initialBrowsePath("./.github/workflows")).toBe(
      "./.github/workflows",
    );
  });
});

describe("path helpers", () => {
  it("finds the parent of a path", () => {
    expect(dirnamePath("/tmp/workflow")).toBe("/tmp");
    expect(dirnamePath("workflow")).toBe(".");
  });

  it("joins a save filename without duplicate slashes", () => {
    expect(joinSave("/tmp/workflows/", "/scratch.yml")).toBe(
      "/tmp/workflows/scratch.yml",
    );
  });

  it("appends the save filename when choosing a directory", () => {
    expect(
      savePathForChoice("/tmp/workflows", "/tmp/workflows", "copy.yml"),
    ).toBe("/tmp/workflows/copy.yml");
    expect(
      savePathForChoice("/tmp/other", "/tmp/workflows", undefined),
    ).toBe("/tmp/other/scratch.yml");
  });

  it("keeps an existing workflow file when choosing it", () => {
    expect(
      savePathForChoice(
        "/tmp/workflows/existing.yaml",
        "/tmp/workflows",
        "copy.yml",
      ),
    ).toBe("/tmp/workflows/existing.yaml");
  });
});

describe("workflowFileName", () => {
  it("takes the YAML basename from a save path", () => {
    expect(workflowFileName("/tmp/workflows/copy.yml")).toBe("copy.yml");
    expect(workflowFileName("scratch.yaml")).toBe("scratch.yaml");
  });

  it("falls back when the path is a directory", () => {
    expect(workflowFileName("/tmp/workflows", "copy.yml")).toBe("copy.yml");
    expect(workflowFileName("/tmp/workflows/")).toBe("scratch.yml");
  });
});

describe("openFolderPath", () => {
  it("keeps a directory as the folder to analyze", () => {
    expect(openFolderPath("/tmp/workflows")).toEqual({
      folder: "/tmp/workflows",
      selectFile: null,
    });
  });

  it("opens the parent folder when a YAML file is chosen", () => {
    expect(openFolderPath("/tmp/workflows/01_happy_path.yml")).toEqual({
      folder: "/tmp/workflows",
      selectFile: "/tmp/workflows/01_happy_path.yml",
    });
  });
});

describe("workflowIndexForPath", () => {
  const workflows = [
    { index: 0, path: "/repo/.github/workflows/a.yml" },
    { index: 1, path: "/repo/.github/workflows/b.yml" },
  ];

  it("matches an exact path, then a basename", () => {
    expect(workflowIndexForPath(workflows, "/repo/.github/workflows/b.yml")).toBe(
      1,
    );
    expect(workflowIndexForPath(workflows, "b.yml")).toBe(1);
    expect(workflowIndexForPath(workflows, "missing.yml")).toBeUndefined();
  });
});
