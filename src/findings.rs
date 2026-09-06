//! Catalog of user-facing findings with plain-language coaching text.
//!
//! Every constructor here fills both `message` (what happened) and `coach`
//! (why it matters and what to try next) so the UI never surfaces a
//! finding without actionable, newbie-friendly guidance.

use std::path::PathBuf;

/// How serious a finding is; drives severity styling in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// What a finding is about: a whole file, a job, or a single step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingTarget {
    File(PathBuf),
    Job {
        file: PathBuf,
        job: String,
    },
    Step {
        file: PathBuf,
        job: String,
        step_idx: usize,
    },
}

/// A single, user-facing diagnostic with a machine-checkable `code` and
/// human coaching text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub code: &'static str,
    pub target: FindingTarget,
    pub message: String,
    pub coach: String,
}

/// A `needs:` dependency cycle among `jobs` was found in `file`; none of
/// the involved jobs can ever start.
pub fn cycle(file: PathBuf, jobs: &[String]) -> Finding {
    let chain = jobs.join(" -> ");
    Finding {
        severity: Severity::Error,
        code: "GHA_CYCLE",
        target: FindingTarget::File(file),
        message: format!("Dependency cycle detected among jobs: {chain}"),
        coach: format!(
            "Jobs {chain} depend on each other in a loop through `needs:`, so none of them can ever run. \
             Remove one of the `needs:` entries or restructure the jobs so dependencies only flow in one direction."
        ),
    }
}

/// Job `job` has a `needs:` entry referencing `missing`, which isn't a job
/// id defined anywhere in `file`.
pub fn missing_need(file: PathBuf, job: impl Into<String>, missing: impl Into<String>) -> Finding {
    let job = job.into();
    let missing = missing.into();
    Finding {
        severity: Severity::Error,
        code: "GHA_MISSING_NEED",
        target: FindingTarget::Job {
            file,
            job: job.clone(),
        },
        message: format!("Job `{job}` needs `{missing}`, which doesn't exist in this workflow"),
        coach: format!(
            "`needs:` must name another job id defined in the same workflow file. \
             Check `{job}` for a typo in `{missing}`, or add a job with that id."
        ),
    }
}

/// `file` could not be parsed as valid GitHub Actions workflow YAML.
pub fn yaml_parse(file: PathBuf, err: impl std::fmt::Display) -> Finding {
    let err = err.to_string();
    Finding {
        severity: Severity::Error,
        code: "GHA_YAML",
        target: FindingTarget::File(file),
        message: format!("Failed to parse workflow YAML: {err}"),
        coach: "This file couldn't be read as a workflow, so none of its jobs can be shown. \
                Open it in an editor with YAML linting, fix the indentation or syntax error near the location above, then reload."
            .to_string(),
    }
}

/// An `${{ ... }}` expression at `target` has malformed syntax (e.g.
/// unbalanced braces).
pub fn expr_syntax(target: FindingTarget, detail: impl Into<String>) -> Finding {
    let detail = detail.into();
    Finding {
        severity: Severity::Warning,
        code: "GHA_EXPR_SYNTAX",
        target,
        message: format!("Expression syntax issue: {detail}"),
        coach: "GitHub Actions expressions must be wrapped in matching double-brace delimiters. \
                Look for a missing closing brace or a stray opening brace nearby and fix the pairing."
            .to_string(),
    }
}

/// `target` uses a construct this tool recognizes but doesn't evaluate yet
/// (v1 scope), for `reason`.
pub fn deferred(target: FindingTarget, reason: impl Into<String>) -> Finding {
    let reason = reason.into();
    Finding {
        severity: Severity::Info,
        code: "GHA_DEFERRED",
        target,
        message: format!("Not evaluated in this version: {reason}"),
        coach: "This construct is recognized but not fully evaluated yet, so the graph and run-state \
                shown for it may be incomplete. Double-check this part manually or in GitHub's own Actions UI."
            .to_string(),
    }
}

/// A modeled construct is visible in the IR/inspector, but gha-see cannot
/// reproduce runner-side behavior during an offline what-if.
pub fn whatif_limit(target: FindingTarget, detail: impl Into<String>) -> Finding {
    let detail = detail.into();
    Finding {
        severity: Severity::Info,
        code: "GHA_WHATIF_LIMIT",
        target,
        message: format!("What-if limit: {detail}"),
        coach: "The workflow structure is modeled and shown in the inspector, but gha-see does not \
                execute jobs, start containers, or simulate GitHub-side queues and protection rules. \
                Verify runtime behavior in GitHub Actions when this detail affects deployment safety."
            .to_string(),
    }
}

/// `write-all` grants every available token permission on a workflow that
/// can run for pull requests.
pub fn permissions_write_all(target: FindingTarget) -> Finding {
    Finding {
        severity: Severity::Warning,
        code: "GHA_PERMISSIONS_WRITE_ALL",
        target,
        message: "`permissions: write-all` is enabled for a pull request workflow".to_string(),
        coach: "A pull request workflow should usually grant only the token capabilities each job \
                needs. Replace `write-all` with an explicit permission map, using `read` or `none` \
                wherever write access is unnecessary."
            .to_string(),
    }
}

/// The current mock push ref is excluded by the workflow's simple branch
/// allow/ignore filters.
pub fn trigger_filter(target: FindingTarget, git_ref: impl Into<String>) -> Finding {
    let git_ref = git_ref.into();
    Finding {
        severity: Severity::Info,
        code: "GHA_TRIGGER_FILTER",
        target,
        message: format!("Mock push ref `{git_ref}` does not match this workflow's branch filters"),
        coach: "This is a best-effort check of `push.branches` and `push.branches-ignore` against \
                the Context sheet ref. GitHub applies additional event payload and filter semantics, \
                so confirm complex patterns in GitHub Actions."
            .to_string(),
    }
}

/// A remote action or reusable workflow reference is intentionally left
/// unresolved because gha-see never performs network access.
pub fn uses_remote(target: FindingTarget, uses: impl Into<String>) -> Finding {
    let uses = uses.into();
    Finding {
        severity: Severity::Info,
        code: "GHA_USES_REMOTE",
        target,
        message: format!("Remote `uses:` reference `{uses}` was not fetched"),
        coach: "Analysis is offline by default, so this reference remains Deferred. In the web UI, \
                use Fetch or Fetch all to explicitly fetch and expand remote definitions, or inspect the pinned \
                repository revision separately."
            .to_string(),
    }
}

/// An explicit fetch failed for one remote action or reusable workflow.
pub fn uses_fetch(
    target: FindingTarget,
    uses: impl Into<String>,
    detail: impl Into<String>,
) -> Finding {
    let uses = uses.into();
    let detail = detail.into();
    Finding {
        severity: Severity::Warning,
        code: "GHA_USES_FETCH",
        target,
        message: format!("Could not fetch remote `uses:` reference `{uses}`: {detail}"),
        coach: "Other jobs were kept and analyzed. Check that the repository, path, and revision \
                exist; for private repositories, set GITHUB_TOKEN or GH_TOKEN and press `f` again."
            .to_string(),
    }
}

/// A local `uses:` path or its expected action descriptor does not exist.
pub fn uses_missing(target: FindingTarget, path: impl Into<String>) -> Finding {
    let path = path.into();
    Finding {
        severity: Severity::Error,
        code: "GHA_USES_MISSING",
        target,
        message: format!("Local `uses:` target `{path}` could not be found"),
        coach: "Local `uses:` paths are resolved from the repository root. Check the spelling and \
                letter case, and ensure a local action directory contains `action.yml` or `action.yaml`."
            .to_string(),
    }
}

/// Reusable workflow calls form a local include cycle.
pub fn workflow_call_cycle(file: PathBuf, chain: &[PathBuf]) -> Finding {
    let display = chain
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" -> ");
    Finding {
        severity: Severity::Error,
        code: "GHA_WORKFLOW_CALL",
        target: FindingTarget::File(file),
        message: format!("Reusable workflow call cycle detected: {display}"),
        coach: "A reusable workflow cannot eventually call itself. Remove one local `uses:` edge \
                from this include chain so workflow expansion terminates."
            .to_string(),
    }
}

/// `target`'s `if:` condition couldn't be resolved against the mock
/// context (unsupported expression shape).
pub fn condition_unknown(target: FindingTarget) -> Finding {
    Finding {
        severity: Severity::Warning,
        code: "GHA_COND_UNKNOWN",
        target,
        message: "Could not determine whether this condition will pass".to_string(),
        coach: "This `if:` condition uses a function or context field this tool's mock evaluator \
                doesn't understand yet. The real outcome depends on the actual trigger, so treat this \
                as unresolved rather than assuming it will run or be skipped."
            .to_string(),
    }
}

/// `job`'s `strategy.matrix` expanded to zero combinations (e.g. every
/// combination was removed by `exclude`, or an axis had no values): no
/// instance of this job will ever run.
pub fn matrix_empty(file: PathBuf, job: impl Into<String>) -> Finding {
    let job = job.into();
    Finding {
        severity: Severity::Warning,
        code: "GHA_MATRIX_EMPTY",
        target: FindingTarget::Job {
            file,
            job: job.clone(),
        },
        message: format!("Job `{job}`'s `strategy.matrix` produces zero combinations"),
        coach: "After applying `exclude` (or because a matrix axis had no values), this job's \
                `strategy.matrix` has no combinations left, so it will never run. Double-check the \
                `matrix`/`exclude` values for a typo or an overly broad exclude rule."
            .to_string(),
    }
}

/// `job`'s `strategy.matrix` expanded to more than `cap` combinations
/// (`total` before truncation); only the first `cap` are shown/evaluated as
/// instances.
pub fn matrix_cap(file: PathBuf, job: impl Into<String>, total: usize, cap: usize) -> Finding {
    let job = job.into();
    Finding {
        severity: Severity::Warning,
        code: "GHA_MATRIX_CAP",
        target: FindingTarget::Job {
            file,
            job: job.clone(),
        },
        message: format!(
            "Job `{job}`'s `strategy.matrix` expands to {total} combinations; only the first {cap} are shown"
        ),
        coach: format!(
            "This tool caps matrix expansion at {cap} instances per job so the graph and inspector stay \
             usable. The remaining combinations aren't shown or evaluated here — reduce the matrix size or \
             check the workflow file directly if you need the full list."
        ),
    }
}

/// `job`'s `strategy.matrix` uses a shape this tool can't expand in v1 (an
/// expression/dynamic value, a non-mapping `strategy`, non-scalar axis
/// values, etc.); the job stays a single `Deferred` node instead of real
/// instances.
pub fn matrix_unsupported(
    file: PathBuf,
    job: impl Into<String>,
    reason: impl Into<String>,
) -> Finding {
    let job = job.into();
    let reason = reason.into();
    Finding {
        severity: Severity::Info,
        code: "GHA_MATRIX_UNSUPPORTED",
        target: FindingTarget::Job {
            file,
            job: job.clone(),
        },
        message: format!("Job `{job}`'s `strategy.matrix` isn't evaluated in this version: {reason}"),
        coach: "This matrix uses a shape (an expression like `fromJSON(...)`, or non-scalar axis values) \
                that this tool doesn't expand yet, so the job is shown as a single deferred node instead of \
                its real instances. Double-check this job's combinations manually or in GitHub's own \
                Actions UI."
            .to_string(),
    }
}

/// `target`'s `if:` condition (`expr`) evaluates to false against the
/// current mock context, so it will be skipped.
pub fn condition_skips(target: FindingTarget, expr: impl Into<String>) -> Finding {
    let expr = expr.into();
    Finding {
        severity: Severity::Info,
        code: "GHA_COND_SKIP",
        target,
        message: format!("Skipped by condition: {expr}"),
        coach: format!(
            "The condition `{expr}` evaluates to false against the current mock context, so this \
             won't run. Adjust the context sheet (event, ref, or env) if you expected it to run."
        ),
    }
}
