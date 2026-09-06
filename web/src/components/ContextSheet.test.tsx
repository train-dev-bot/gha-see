import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { EvalContextDto } from "../types";
import ContextSheet, {
  inputsTextForWorkflow,
  mergeWorkflowInputs,
} from "./ContextSheet";

function stubContext(over: Partial<EvalContextDto> = {}): EvalContextDto {
  return {
    eventName: "push",
    refName: "refs/heads/main",
    github: { sha: "abc", repository: "o/r", actor: "octocat" },
    env: {},
    vars: {},
    inputs: {},
    secrets: [],
    needs: {},
    repoRoot: null,
    ...over,
  };
}

describe("inputsTextForWorkflow", () => {
  it("uses context values then defaults for the active workflow only", () => {
    const text = inputsTextForWorkflow(
      [
        { name: "environment", default: "staging", required: false },
        { name: "skip_dast", default: "false", required: false },
      ],
      { environment: "prod", foreign: "nope" },
    );
    expect(text).toBe("environment=prod\nskip_dast=false");
    expect(text).not.toContain("foreign");
  });
});

describe("mergeWorkflowInputs", () => {
  it("updates only the active workflow keys", () => {
    expect(
      mergeWorkflowInputs(
        { environment: "staging", skip_dast: "false", dry_run: "true" },
        ["environment", "skip_dast"],
        { environment: "prod", skip_dast: "true" },
      ),
    ).toEqual({
      environment: "prod",
      skip_dast: "true",
      dry_run: "true",
    });
  });

  it("keeps ad-hoc key=value lines when the workflow declares no inputs", () => {
    expect(
      mergeWorkflowInputs({ environment: "staging" }, [], {
        foo: "bar",
      }),
    ).toEqual({
      environment: "staging",
      foo: "bar",
    });
  });
});

describe("ContextSheet", () => {
  it("lists push and pull_request as trigger events", () => {
    render(
      <ContextSheet
        context={stubContext()}
        onApply={() => {}}
        applying={false}
      />,
    );
    const select = screen.getByLabelText("Trigger event");
    const values = Array.from(select.querySelectorAll("option")).map((opt) =>
      opt.getAttribute("value"),
    );
    expect(values).toContain("push");
    expect(values).toContain("pull_request");
  });

  it("calls onApply with the new eventName", async () => {
    const user = userEvent.setup();
    const onApply = vi.fn();
    render(
      <ContextSheet
        context={stubContext({ eventName: "push" })}
        onApply={onApply}
        applying={false}
      />,
    );
    await user.selectOptions(
      screen.getByLabelText("Trigger event"),
      "pull_request",
    );
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(onApply).toHaveBeenCalled();
    expect(onApply.mock.calls[0][0].eventName).toBe("pull_request");
  });

  it("disables Apply while applying", () => {
    render(
      <ContextSheet
        context={stubContext()}
        onApply={() => {}}
        applying={true}
      />,
    );
    expect(screen.getByRole("button", { name: "Applying…" })).toHaveProperty(
      "disabled",
      true,
    );
  });
});
