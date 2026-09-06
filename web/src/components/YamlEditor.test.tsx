import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import YamlEditor from "./YamlEditor";

const SOURCE = "name: demo\non: push\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n";

function renderEditor(
  overrides: {
    initialSource?: string;
    onAnalyze?: (name: string, source: string) => Promise<void>;
    onSave?: (path: string, source: string) => Promise<void>;
    onClose?: () => void;
  } = {},
) {
  const onAnalyze = overrides.onAnalyze ?? vi.fn().mockResolvedValue(undefined);
  const onSave = overrides.onSave ?? vi.fn().mockResolvedValue(undefined);
  const onClose = overrides.onClose ?? vi.fn();
  render(
    <YamlEditor
      initialSource={overrides.initialSource ?? SOURCE}
      suggestedName="demo.yml"
      suggestedSavePath="./.github/workflows/demo.yml"
      busy={false}
      onAnalyze={onAnalyze}
      onSave={onSave}
      onClose={onClose}
    />,
  );
  return { onAnalyze, onSave, onClose };
}

describe("YamlEditor", () => {
  it("calls onAnalyze with the current source", async () => {
    const user = userEvent.setup();
    const { onAnalyze } = renderEditor();
    await user.click(screen.getByRole("button", { name: "Analyze" }));
    expect(onAnalyze).toHaveBeenCalledWith("demo.yml", SOURCE);
  });

  it("calls onSave", async () => {
    const user = userEvent.setup();
    const { onSave } = renderEditor();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalledWith(
      "./.github/workflows/demo.yml",
      SOURCE,
    );
  });

  it("calls onClose", async () => {
    const user = userEvent.setup();
    const { onClose } = renderEditor();
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("shows unsaved status after typing", async () => {
    const user = userEvent.setup();
    renderEditor();
    await user.type(
      screen.getByPlaceholderText("Paste workflow YAML here…"),
      " ",
    );
    expect(screen.getByRole("status").textContent).toMatch(/Unsaved changes/);
  });

  it("disables Analyze when source is whitespace", async () => {
    const user = userEvent.setup();
    renderEditor({ initialSource: SOURCE });
    await user.clear(screen.getByPlaceholderText("Paste workflow YAML here…"));
    expect(screen.getByRole("button", { name: "Analyze" })).toHaveProperty(
      "disabled",
      true,
    );
  });
});
