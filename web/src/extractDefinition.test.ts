import { describe, expect, it } from "vitest";
import { extractJobYaml, extractStepYaml } from "./extractDefinition";

const SRC = `name: Demo
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Test
        run: cargo test
  deploy:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - run: echo go
`;

describe("extractJobYaml", () => {
  it("returns the build job mapping", () => {
    const y = extractJobYaml(SRC, "build");
    expect(y).toContain("runs-on: ubuntu-latest");
    expect(y).toContain("cargo test");
    expect(y).not.toContain("needs: build");
  });
  it("extracts a normal job after a quoted job id", () => {
    const source = `jobs:
  "weird-job":
    runs-on: ubuntu-latest
  build:
    runs-on: ubuntu-latest
`;

    expect(extractJobYaml(source, "build")).toBe(`  build:
    runs-on: ubuntu-latest
`);
  });
  it("stops before a quoted sibling job id", () => {
    const source = `jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: npm test
  "weird-job":
    runs-on: windows-latest
    steps:
      - run: echo weird
  deploy:
    needs: build
`;

    const y = extractJobYaml(source, "build");
    expect(y).toContain("runs-on: ubuntu-latest");
    expect(y).toContain("npm test");
    expect(y).not.toContain("windows-latest");
    expect(y).not.toContain("echo weird");
  });
  it("returns null for unknown job", () => {
    expect(extractJobYaml(SRC, "missing")).toBeNull();
  });
});

describe("extractStepYaml", () => {
  it("returns step 1 of build", () => {
    const y = extractStepYaml(SRC, "build", 1);
    expect(y).toContain("name: Test");
    expect(y).toContain("cargo test");
    expect(y).not.toContain("actions/checkout");
  });
  it("returns null when slice fails", () => {
    expect(extractStepYaml(SRC, "build", 99)).toBeNull();
  });
});
