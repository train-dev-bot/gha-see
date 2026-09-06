//! Scan `${{ ... }}` expressions embedded in job/step fields, without
//! evaluating them, to surface two things: syntax problems (unbalanced
//! delimiters) and `needs.<job>.outputs.<name>` bindings that describe how
//! job outputs flow between jobs.
//!
//! This layer is deliberately dumb: it does not parse GHA expression syntax
//! or evaluate anything (that's `eval.rs`'s job). It only does textual
//! scanning, so it's simple, deterministic, and safe to run on every field
//! without a network or shell.

use crate::findings::{expr_syntax, Finding, FindingTarget};
use crate::ir::WorkflowFile;

/// A traced `needs.<producer_job>.outputs.<output_name>` reference found
/// inside an expression belonging to `consumer_job`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub consumer_job: String,
    /// `None` for a job-level field, otherwise the consuming step's index.
    pub consumer_step_idx: Option<usize>,
    pub producer_job: String,
    pub output_name: String,
    pub raw: String,
}

/// Extract the trimmed inner contents of every well-formed `${{ ... }}`
/// expression in `s`, in order of appearance.
///
/// Only complete pairs are returned: an unmatched `${{` with no following
/// `}}` is skipped here (see [`unbalanced_expression`] to detect that case
/// instead of silently dropping it).
pub fn extract_expressions(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;

    while let Some(open) = rest.find("${{") {
        let after_open = &rest[open + 3..];
        match after_open.find("}}") {
            Some(close) => {
                out.push(after_open[..close].trim().to_string());
                rest = &after_open[close + 2..];
            }
            None => break,
        }
    }

    out
}

/// Report whether `s` contains an unbalanced `${{`/`}}` pairing: an opening
/// delimiter with no matching close, a closing delimiter with no matching
/// open, or any other mismatch between the number of opens and closes.
pub fn unbalanced_expression(s: &str) -> bool {
    let mut rest = s;
    let mut depth: i32 = 0;

    loop {
        let open_pos = rest.find("${{");
        let close_pos = rest.find("}}");

        match (open_pos, close_pos) {
            (None, None) => break,
            (Some(o), None) => {
                depth += 1;
                rest = &rest[o + 3..];
            }
            (None, Some(c)) => {
                depth -= 1;
                rest = &rest[c + 2..];
            }
            (Some(o), Some(c)) => {
                if o < c {
                    depth += 1;
                    rest = &rest[o + 3..];
                } else {
                    depth -= 1;
                    rest = &rest[c + 2..];
                }
            }
        }
    }

    depth != 0
}

/// Scan every job/step field capable of holding an expression (job `if`,
/// job `env`/`outputs`/`strategy` values, step `if`/`run`/`uses`/`with`/`env`) across `wf`'s
/// **instances**, tracing `needs.*.outputs.*` bindings and flagging
/// unbalanced `${{ }}` syntax.
///
/// Iterating instances (rather than logical jobs) means a matrix job's
/// bindings/findings are attributed to each of its instance ids — matching
/// how the TUI selects rows and `AnalysisView::job_states`/`step_states`
/// are keyed — even though `consumer_job`/instance text is identical across
/// every instance of the same base job (they clone the same `Job` fields).
/// `Binding::producer_job` stays a *base* job id regardless, since that's
/// what `needs.<job>.outputs.<name>` always names in the YAML.
///
/// A field with unbalanced syntax is reported as a [`Finding`] and skipped
/// for binding extraction (there's nothing reliable to extract from
/// malformed delimiters).
pub fn scan_workflow(wf: &WorkflowFile) -> (Vec<Binding>, Vec<Finding>) {
    let mut bindings = Vec::new();
    let mut findings = Vec::new();

    for (instance_id, instance) in &wf.instances {
        let job_target = FindingTarget::Job {
            file: wf.path.clone(),
            job: instance_id.clone(),
        };

        if let Some(condition) = &instance.condition {
            scan_field(
                condition,
                instance_id,
                None,
                job_target.clone(),
                &mut bindings,
                &mut findings,
            );
        }
        for value in instance.env.values() {
            scan_field(
                value,
                instance_id,
                None,
                job_target.clone(),
                &mut bindings,
                &mut findings,
            );
        }
        for value in instance.outputs.values() {
            scan_field(
                value,
                instance_id,
                None,
                job_target.clone(),
                &mut bindings,
                &mut findings,
            );
        }
        if let Some(strategy) = &instance.strategy {
            if let Ok(value) = serde_yaml::to_string(strategy) {
                scan_field(
                    &value,
                    instance_id,
                    None,
                    job_target.clone(),
                    &mut bindings,
                    &mut findings,
                );
            }
        }

        for (step_idx, step) in instance.steps.iter().enumerate() {
            let step_target = FindingTarget::Step {
                file: wf.path.clone(),
                job: instance_id.clone(),
                step_idx,
            };

            if let Some(condition) = &step.condition {
                scan_field(
                    condition,
                    instance_id,
                    Some(step_idx),
                    step_target.clone(),
                    &mut bindings,
                    &mut findings,
                );
            }
            if let Some(run) = &step.run {
                scan_field(
                    run,
                    instance_id,
                    Some(step_idx),
                    step_target.clone(),
                    &mut bindings,
                    &mut findings,
                );
            }
            if let Some(uses) = &step.uses {
                scan_field(
                    uses,
                    instance_id,
                    Some(step_idx),
                    step_target.clone(),
                    &mut bindings,
                    &mut findings,
                );
            }
            for value in step.with_inputs.values() {
                scan_field(
                    value,
                    instance_id,
                    Some(step_idx),
                    step_target.clone(),
                    &mut bindings,
                    &mut findings,
                );
            }
            for value in step.env.values() {
                scan_field(
                    value,
                    instance_id,
                    Some(step_idx),
                    step_target.clone(),
                    &mut bindings,
                    &mut findings,
                );
            }
        }
    }

    (bindings, findings)
}

/// Scan a single raw field value for syntax problems and bindings, pushing
/// results into `bindings`/`findings` as appropriate.
fn scan_field(
    raw: &str,
    consumer_job: &str,
    consumer_step_idx: Option<usize>,
    target: FindingTarget,
    bindings: &mut Vec<Binding>,
    findings: &mut Vec<Finding>,
) {
    if unbalanced_expression(raw) {
        findings.push(expr_syntax(
            target,
            format!("unmatched `${{{{` near: {raw}"),
        ));
        return;
    }

    for expr in extract_expressions(raw) {
        bindings.extend(needs_outputs_bindings(
            &expr,
            consumer_job,
            consumer_step_idx,
        ));
    }
}

/// Find every `needs.<job>.outputs.<name>` reference inside a single
/// (already-unwrapped) expression body, mirroring the character class
/// `[A-Za-z0-9_-]+` for both the job id and output name.
fn needs_outputs_bindings(
    expr: &str,
    consumer_job: &str,
    consumer_step_idx: Option<usize>,
) -> Vec<Binding> {
    const PREFIX: &str = "needs.";
    const MIDDLE: &str = ".outputs.";

    let mut out = Vec::new();
    let mut search_start = 0;

    while let Some(rel) = expr[search_start..].find(PREFIX) {
        let start = search_start + rel;
        let after_prefix = &expr[start + PREFIX.len()..];

        if let Some((producer_job, after_producer)) = take_ident(after_prefix) {
            if let Some(after_middle) = after_producer.strip_prefix(MIDDLE) {
                if let Some((output_name, _rest)) = take_ident(after_middle) {
                    let raw_len =
                        PREFIX.len() + producer_job.len() + MIDDLE.len() + output_name.len();
                    out.push(Binding {
                        consumer_job: consumer_job.to_string(),
                        consumer_step_idx,
                        producer_job,
                        output_name,
                        raw: expr[start..start + raw_len].to_string(),
                    });
                    search_start = start + raw_len;
                    continue;
                }
            }
        }

        search_start = start + PREFIX.len();
    }

    out
}

/// Consume a leading run of `[A-Za-z0-9_-]` characters from `s`, returning
/// the identifier and the remainder. `None` if `s` doesn't start with at
/// least one such character.
fn take_ident(s: &str) -> Option<(String, &str)> {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(s.len());
    if end == 0 {
        None
    } else {
        Some((s[..end].to_string(), &s[end..]))
    }
}
