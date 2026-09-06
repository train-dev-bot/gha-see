import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import AppHeader from "./AppHeader";

const noop = () => {};

function renderHeader(
  overrides: Partial<{
    pendingFetchCount: number;
    showEditor: boolean;
    editorDirty: boolean;
  }> = {},
) {
  return render(
    <AppHeader
      pathLabel="/repo"
      showScratchBadge={false}
      reloading={false}
      issueCount={2}
      skippedCount={1}
      pendingFetchCount={overrides.pendingFetchCount ?? 0}
      fetchingAll={false}
      showEditor={overrides.showEditor ?? false}
      editorDirty={overrides.editorDirty ?? false}
      onReload={noop}
      onBrowse={noop}
      onFetchAll={noop}
      onToggleEditor={noop}
    />,
  );
}

describe("AppHeader", () => {
  it("renders gha-see", () => {
    renderHeader();
    expect(screen.getByText("gha-see")).toBeTruthy();
  });

  it("disables Fetch all when pendingFetchCount is 0", () => {
    renderHeader({ pendingFetchCount: 0 });
    expect(screen.getByRole("button", { name: "Fetch all" })).toHaveProperty(
      "disabled",
      true,
    );
  });

  it("marks View & edit YAML pressed when the editor is open", () => {
    renderHeader({ showEditor: true });
    expect(
      screen.getByRole("button", { name: "View & edit YAML" }).getAttribute("aria-pressed"),
    ).toBe("true");
  });

  it("appends unsaved to the YAML button when dirty", () => {
    renderHeader({ editorDirty: true });
    expect(
      screen.getByRole("button", { name: "View & edit YAML · unsaved" }),
    ).toBeTruthy();
  });
});
