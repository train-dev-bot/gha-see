//! Orchestrate the full analysis pipeline: discover workflow files, parse and
//! normalize each one, build job graphs, trace `needs.*.outputs.*` bindings,
//! and evaluate run states — collecting every [`Finding`] raised along the
//! way into a single [`AnalysisView`] for the TUI to render.
//!
//! Two entry points cover the two things a caller needs to do:
//! - [`analyze_path`] runs the full ("cold") pipeline once, from a file or
//!   directory path down to evaluated run states.
//! - [`revaluate`] re-runs only the condition-evaluation ("hot") step
//!   against a different [`EvalContext`] (e.g. the user editing the mock
//!   context sheet), without re-parsing or rebuilding the structural
//!   (YAML/graph/binding) findings.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::discover::{discover_workflows, DiscoverError};
use crate::eval::{evaluate, EvalContext, RunState};
use crate::expr::{scan_workflow, Binding};
use crate::fetch::{default_cache_root, ensure_cached, FetchOutcome, RemoteRef};
use crate::findings::{
    deferred, matrix_cap, matrix_empty, matrix_unsupported, permissions_write_all, trigger_filter,
    uses_fetch, whatif_limit, yaml_parse, Finding, FindingTarget,
};
use crate::graph::{build_job_graph, JobGraph};
use crate::ir::{normalize, Permissions, SupportTier, WorkflowFile};
use crate::matrix::MatrixNote;
use crate::parse::parse_workflow_str;
use crate::uses::{resolve_workflow, resolve_workflow_with_cache};

/// Everything the analysis pipeline produced for a discovered set of
/// workflow files: the normalized workflows themselves, their job graphs,
/// traced output bindings, every [`Finding`] raised, and the resolved run
/// state of each job/step under the last-used [`EvalContext`].
///
/// `bindings` and the two state maps are keyed (in part) by the source
/// file's [`PathBuf`] since job ids are only unique within a single
/// workflow file, not across the whole discovered set. The `String` half of
/// `job_states`/`step_states`' keys is an **instance id**
/// (`crate::ir::JobInstance::instance_id`) — equal to the plain job id for
/// jobs without `strategy.matrix`, or `"<job> (<axis>=<value>, ...)"` for a
/// matrix combination.
#[derive(Clone)]
pub struct AnalysisView {
    pub workflows: Vec<WorkflowFile>,
    pub graphs: Vec<JobGraph>,
    pub bindings: Vec<(PathBuf, Binding)>,
    pub findings: Vec<Finding>,
    pub job_states: BTreeMap<(PathBuf, String), RunState>,
    pub step_states: BTreeMap<(PathBuf, String, usize), RunState>,
}

/// Fatal error from [`analyze_path`]: only discovery (bad root path) can
/// fail outright. A workflow file that fails to parse does *not* fail the
/// whole analysis — see [`analyze_path`]'s docs.
#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Discover(#[from] DiscoverError),
    #[error("workflow index {0} does not exist")]
    WorkflowIndex(usize),
    #[error("could not determine the user cache directory")]
    CacheDirectory,
}

/// Run the full ("cold") analysis pipeline over `path` (a single workflow
/// file, or a directory searched via [`discover_workflows`]).
///
/// Pipeline, in order:
/// 1. **Discover** every workflow file under `path`. A failure here (bad
///    root path) is the only way this function returns `Err`.
/// 2. **Parse + normalize** each file. A YAML parse failure does not abort
///    the run: it's recorded as a `GHA_YAML` [`Finding`] and the file is
///    represented by a stub [`WorkflowFile`] (`parse_ok: false`, no jobs)
///    so the rest of the set still analyzes normally.
/// 3. **Deferred + matrix findings**: every `SupportTier::Deferred` job/step
///    (tagged by `ir::normalize`) becomes a `GHA_DEFERRED` finding, and
///    every job's `strategy.matrix` outcome (recorded by
///    `matrix::expand_workflow` as `Job::matrix_note`) becomes a
///    `GHA_MATRIX_EMPTY`/`GHA_MATRIX_CAP`/`GHA_MATRIX_UNSUPPORTED` finding.
/// 4. **Graph findings**: `graph::build_job_graph` per workflow (nodes are
///    job **instances**; a `needs:` on a matrix job fans out to every
///    instance), surfacing
///    `GHA_CYCLE` / `GHA_MISSING_NEED`.
/// 5. **Expression scan**: `expr::scan_workflow` per workflow, surfacing
///    `GHA_EXPR_SYNTAX` and tracing `needs.*.outputs.*` [`Binding`]s.
/// 6. **Evaluate**: `eval::evaluate` per workflow against
///    `EvalContext::default_mock()`, surfacing `GHA_COND_SKIP` /
///    `GHA_COND_UNKNOWN` and populating `job_states` / `step_states`.
pub fn analyze_path(path: &Path) -> Result<AnalysisView, AnalyzeError> {
    let paths = discover_workflows(path)?;
    // Read the local uses cache when present (still offline — no network).
    // Remotes already fetched earlier resolve immediately; Fetch only downloads misses.
    let cache = default_cache_root();
    Ok(analyze_files(&paths, path, cache.as_deref(), Vec::new()))
}

/// Analyze an in-memory YAML workflow as a scratch file without writing it to
/// the filesystem.
pub fn analyze_yaml_source(label: &str, source: &str) -> AnalysisView {
    let safe_label: String = label
        .chars()
        .filter(|character| *character != '/' && *character != '\\')
        .collect();
    let path = PathBuf::from(format!("(scratch)/{safe_label}"));
    let mut findings = Vec::new();
    let workflow = match parse_workflow_str(source) {
        Ok(raw) => {
            let mut workflow = normalize(path.clone(), raw);
            workflow.raw_source = Some(source.to_string());
            if let Some(cache_root) = default_cache_root() {
                findings.extend(resolve_workflow_with_cache(
                    &mut workflow,
                    Path::new("."),
                    &cache_root,
                ));
            } else {
                findings.extend(resolve_workflow(&mut workflow, Path::new(".")));
            }
            workflow
        }
        Err(error) => {
            findings.push(yaml_parse(path.clone(), &error));
            stub_workflow(path, error.to_string(), Some(source.to_string()))
        }
    };

    finish_analysis(vec![workflow], findings)
}

fn analyze_files(
    paths: &[PathBuf],
    analyzed_path: &Path,
    cache_root: Option<&Path>,
    mut findings: Vec<Finding>,
) -> AnalysisView {
    let mut workflows = Vec::with_capacity(paths.len());

    for file_path in paths {
        match fs::read_to_string(file_path) {
            Ok(raw_source) => match parse_workflow_str(&raw_source) {
                Ok(raw) => {
                    let repo_root = repository_root(file_path, analyzed_path);
                    let mut workflow = normalize(file_path.clone(), raw);
                    workflow.raw_source = Some(raw_source);
                    if let Some(cache_root) = cache_root {
                        findings.extend(resolve_workflow_with_cache(
                            &mut workflow,
                            &repo_root,
                            cache_root,
                        ));
                    } else {
                        findings.extend(resolve_workflow(&mut workflow, &repo_root));
                    }
                    workflows.push(workflow);
                }
                Err(err) => {
                    findings.push(yaml_parse(file_path.clone(), &err));
                    workflows.push(stub_workflow(
                        file_path.clone(),
                        err.to_string(),
                        Some(raw_source),
                    ));
                }
            },
            Err(err) => {
                let message = format!(
                    "failed to read workflow file {}: {err}",
                    file_path.display()
                );
                findings.push(yaml_parse(file_path.clone(), &message));
                workflows.push(stub_workflow(file_path.clone(), message, None));
            }
        }
    }

    finish_analysis(workflows, findings)
}

fn finish_analysis(workflows: Vec<WorkflowFile>, mut findings: Vec<Finding>) -> AnalysisView {
    for wf in &workflows {
        collect_deferred_findings(wf, &mut findings);
        collect_matrix_findings(wf, &mut findings);
        collect_extra_findings(wf, &mut findings);
    }

    let mut graphs = Vec::with_capacity(workflows.len());
    let mut bindings = Vec::new();
    let ctx = EvalContext::default_mock_for(&workflows);
    let mut job_states = BTreeMap::new();
    let mut step_states = BTreeMap::new();

    for wf in &workflows {
        let (graph, graph_findings) = build_job_graph(wf);
        findings.extend(graph_findings);
        graphs.push(graph);

        let (wf_bindings, expr_findings) = scan_workflow(wf);
        findings.extend(expr_findings);
        bindings.extend(wf_bindings.into_iter().map(|b| (wf.path.clone(), b)));

        insert_run_states(wf, &ctx, &mut job_states, &mut step_states, &mut findings);
    }

    AnalysisView {
        workflows,
        graphs,
        bindings,
        findings,
        job_states,
        step_states,
    }
}

/// Local `uses: ./…` paths are rooted at the repository, not beside the
/// workflow file. Prefer the parent of a `.github` ancestor; otherwise use
/// the analyzed directory (or the explicit file's parent) as a best effort.
fn repository_root(file: &Path, analyzed_path: &Path) -> PathBuf {
    for ancestor in file.ancestors() {
        if ancestor.file_name().and_then(|name| name.to_str()) == Some(".github") {
            if let Some(parent) = ancestor.parent() {
                return parent.to_path_buf();
            }
        }
    }
    if analyzed_path.is_dir() {
        analyzed_path.to_path_buf()
    } else {
        analyzed_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }
}

/// Whether the workflow still contains a GitHub-hosted `uses:` string
/// (`owner/repo@ref`), regardless of whether it has already been fetched
/// into the local cache.
pub fn workflow_has_remote_uses(workflow: &WorkflowFile) -> bool {
    !remote_uses(workflow).is_empty()
}

/// Whether any remote `uses:` is still unresolved (not loaded from cache).
/// Used by the web UI to show/hide the Fetch button — after a successful
/// fetch + resolve, this becomes `false` even though the YAML still names
/// remote actions.
pub fn workflow_needs_remote_fetch(workflow: &WorkflowFile) -> bool {
    for job in workflow.jobs.values() {
        if let Some(uses) = &job.uses {
            if RemoteRef::parse(uses).is_ok() && job.support == SupportTier::Deferred {
                return true;
            }
        }
        if steps_need_remote_fetch(&job.steps) {
            return true;
        }
    }
    for instance in workflow.instances.values() {
        if steps_need_remote_fetch(&instance.steps) {
            return true;
        }
    }
    false
}

fn steps_need_remote_fetch(steps: &[crate::ir::Step]) -> bool {
    for step in steps {
        if let Some(uses) = &step.uses {
            if RemoteRef::parse(uses).is_ok() && step.resolved_action_name.is_none() {
                return true;
            }
        }
        if steps_need_remote_fetch(&step.composite_steps) {
            return true;
        }
    }
    false
}

/// Explicitly fetch all remote uses reachable from one workflow, then
/// rebuild the analysis resolving **every** workflow against the user cache
/// (so sibling workflows hide Fetch once their remotes are cached).
/// Ordinary [`analyze_path`] calls never open the network; they only read
/// the cache if it already exists.
pub fn refetch_workflow_remotes(
    view: &AnalysisView,
    workflow_idx: usize,
) -> Result<AnalysisView, AnalyzeError> {
    let cache_root = default_cache_root().ok_or(AnalyzeError::CacheDirectory)?;
    refetch_workflow_remotes_with(view, Some(workflow_idx), &cache_root, |remote, root| {
        ensure_cached(remote, root).unwrap_or_else(|error| FetchOutcome::Failed {
            remote: remote.clone(),
            error: error.to_string(),
        })
    })
}

/// Fetch remotes for every workflow that still needs them, then rebuild.
pub fn refetch_all_remotes(view: &AnalysisView) -> Result<AnalysisView, AnalyzeError> {
    let cache_root = default_cache_root().ok_or(AnalyzeError::CacheDirectory)?;
    refetch_workflow_remotes_with(view, None, &cache_root, |remote, root| {
        ensure_cached(remote, root).unwrap_or_else(|error| FetchOutcome::Failed {
            remote: remote.clone(),
            error: error.to_string(),
        })
    })
}

/// Fetcher-injected variant for cache-seeded and soft-failure tests.
/// `workflow_idx` of `None` means fetch remotes for all workflows.
pub fn refetch_workflow_remotes_with<F>(
    view: &AnalysisView,
    workflow_idx: Option<usize>,
    cache_root: &Path,
    mut fetcher: F,
) -> Result<AnalysisView, AnalyzeError>
where
    F: FnMut(&RemoteRef, &Path) -> FetchOutcome,
{
    let workflows: Vec<&WorkflowFile> = match workflow_idx {
        Some(idx) => {
            let workflow = view
                .workflows
                .get(idx)
                .ok_or(AnalyzeError::WorkflowIndex(idx))?;
            vec![workflow]
        }
        None => view.workflows.iter().collect(),
    };

    let mut queue = VecDeque::new();
    for workflow in &workflows {
        for (remote, target) in remote_uses(workflow) {
            queue.push_back((remote, target, 0usize));
        }
    }

    let mut outcomes = BTreeMap::<RemoteRef, FetchOutcome>::new();
    let mut expanded = BTreeSet::<RemoteRef>::new();
    let mut fetch_findings = Vec::new();

    while let Some((remote, target, depth)) = queue.pop_front() {
        if depth >= 8 {
            continue;
        }
        let outcome = outcomes
            .entry(remote.clone())
            .or_insert_with(|| fetcher(&remote, cache_root))
            .clone();
        match outcome {
            FetchOutcome::Failed { error, .. } => {
                fetch_findings.push(uses_fetch(target, remote.raw, error));
            }
            FetchOutcome::Cached { .. } | FetchOutcome::Fetched { .. } => {
                if expanded.insert(remote.clone()) {
                    for nested in cached_nested_remotes(&remote, cache_root) {
                        queue.push_back((nested, target.clone(), depth + 1));
                    }
                }
            }
        }
    }

    let paths = view
        .workflows
        .iter()
        .map(|workflow| workflow.path.clone())
        .collect::<Vec<_>>();
    let analyzed_path = paths
        .first()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(analyze_files(
        &paths,
        &analyzed_path,
        Some(cache_root),
        fetch_findings,
    ))
}

fn remote_uses(workflow: &WorkflowFile) -> Vec<(RemoteRef, FindingTarget)> {
    let mut remotes = Vec::new();
    for (job_id, job) in &workflow.jobs {
        if let Some(uses) = &job.uses {
            if let Ok(remote) = RemoteRef::parse(uses) {
                remotes.push((
                    remote,
                    FindingTarget::Job {
                        file: workflow.path.clone(),
                        job: job_id.clone(),
                    },
                ));
            }
        }
        for (step_idx, step) in job.steps.iter().enumerate() {
            let target = FindingTarget::Step {
                file: workflow.path.clone(),
                job: job_id.clone(),
                step_idx,
            };
            collect_step_remotes(step, &target, &mut remotes);
        }
    }
    remotes
}

fn collect_step_remotes(
    step: &crate::ir::Step,
    target: &FindingTarget,
    remotes: &mut Vec<(RemoteRef, FindingTarget)>,
) {
    if let Some(uses) = &step.uses {
        if let Ok(remote) = RemoteRef::parse(uses) {
            remotes.push((remote, target.clone()));
        }
    }
    for child in &step.composite_steps {
        collect_step_remotes(child, target, remotes);
    }
}

fn cached_nested_remotes(remote: &RemoteRef, cache_root: &Path) -> Vec<RemoteRef> {
    let target = remote.target_path(cache_root);
    let yaml_path = if target.is_file() {
        Some(target)
    } else {
        ["action.yml", "action.yaml"]
            .into_iter()
            .map(|name| target.join(name))
            .find(|path| path.is_file())
    };
    let Some(path) = yaml_path else {
        return Vec::new();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(&contents) else {
        return Vec::new();
    };
    let mut remotes = Vec::new();
    collect_yaml_remotes(&yaml, &mut remotes);
    remotes
}

fn collect_yaml_remotes(value: &serde_yaml::Value, remotes: &mut Vec<RemoteRef>) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            for (key, value) in mapping {
                if key.as_str() == Some("uses") {
                    if let Some(uses) = value.as_str() {
                        if let Ok(remote) = RemoteRef::parse(uses) {
                            remotes.push(remote);
                        }
                    }
                }
                collect_yaml_remotes(value, remotes);
            }
        }
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                collect_yaml_remotes(value, remotes);
            }
        }
        _ => {}
    }
}

/// Re-run only the condition-evaluation ("hot") step of the pipeline
/// against a different [`EvalContext`] — e.g. the user flipping the mock
/// context sheet's event/ref/env — without re-discovering, re-parsing, or
/// rebuilding graphs/bindings from scratch.
///
/// Structural findings (`GHA_YAML`, `GHA_CYCLE`, `GHA_MISSING_NEED`,
/// `GHA_EXPR_SYNTAX`, `GHA_DEFERRED`) are carried over unchanged from
/// `view`; only the condition findings (`GHA_COND_SKIP`,
/// `GHA_COND_UNKNOWN`) and the run-state maps are replaced with freshly
/// evaluated ones.
pub fn revaluate(view: &AnalysisView, ctx: &EvalContext) -> AnalysisView {
    let mut findings: Vec<Finding> = view
        .findings
        .iter()
        .filter(|f| !is_condition_finding(f))
        .cloned()
        .collect();

    // `JobGraph` isn't `Clone` (it wraps a `petgraph::Graph`), so the graph
    // structure is rebuilt from the unchanged `workflows` rather than
    // cloned field-by-field. The workflows haven't changed, so this yields
    // an identical graph; its findings are discarded here since the
    // originals are already preserved above.
    let mut graphs = Vec::with_capacity(view.workflows.len());
    let mut job_states = BTreeMap::new();
    let mut step_states = BTreeMap::new();

    for wf in &view.workflows {
        let (graph, _already_recorded) = build_job_graph(wf);
        graphs.push(graph);

        insert_run_states(wf, ctx, &mut job_states, &mut step_states, &mut findings);
    }

    AnalysisView {
        workflows: view.workflows.clone(),
        graphs,
        bindings: view.bindings.clone(),
        findings,
        job_states,
        step_states,
    }
}

fn is_condition_finding(finding: &Finding) -> bool {
    matches!(
        finding.code,
        "GHA_COND_SKIP" | "GHA_COND_UNKNOWN" | "GHA_TRIGGER_FILTER"
    )
}

/// A placeholder [`WorkflowFile`] for a file that failed to parse: no jobs,
/// `parse_ok: false`, with `err` kept around for diagnostics/rendering.
fn stub_workflow(path: PathBuf, err: String, raw_source: Option<String>) -> WorkflowFile {
    WorkflowFile {
        path,
        raw_source,
        name: None,
        on: serde_yaml::Value::Null,
        triggers: Default::default(),
        concurrency: None,
        permissions: None,
        defaults_run: None,
        jobs: BTreeMap::new(),
        instances: BTreeMap::new(),
        parse_ok: false,
        file_error: Some(err),
    }
}

/// Turn every `SupportTier::Deferred` job/step's `deferred_reasons` (set by
/// `ir::normalize`) into `GHA_DEFERRED` findings.
fn collect_deferred_findings(wf: &WorkflowFile, findings: &mut Vec<Finding>) {
    for (job_id, job) in &wf.jobs {
        let job_target = FindingTarget::Job {
            file: wf.path.clone(),
            job: job_id.clone(),
        };
        for reason in &job.deferred_reasons {
            findings.push(deferred(job_target.clone(), reason.clone()));
        }

        for (step_idx, step) in job.steps.iter().enumerate() {
            if step.deferred_reasons.is_empty() {
                continue;
            }
            let step_target = FindingTarget::Step {
                file: wf.path.clone(),
                job: job_id.clone(),
                step_idx,
            };
            for reason in &step.deferred_reasons {
                findings.push(deferred(step_target.clone(), reason.clone()));
            }
        }
    }
}

/// Turn every job's `Job::matrix_note` (set by `matrix::expand_workflow`)
/// into the matching `GHA_MATRIX_*` finding, targeting the base job id
/// (matrix findings describe the whole expansion, not one instance).
fn collect_matrix_findings(wf: &WorkflowFile, findings: &mut Vec<Finding>) {
    for (job_id, job) in &wf.jobs {
        match &job.matrix_note {
            None => {}
            Some(MatrixNote::Empty) => {
                findings.push(matrix_empty(wf.path.clone(), job_id.clone()));
            }
            Some(MatrixNote::Capped { total, cap }) => {
                findings.push(matrix_cap(wf.path.clone(), job_id.clone(), *total, *cap));
            }
            Some(MatrixNote::Unsupported(reason)) => {
                findings.push(matrix_unsupported(
                    wf.path.clone(),
                    job_id.clone(),
                    reason.clone(),
                ));
            }
        }
    }
}

fn collect_extra_findings(wf: &WorkflowFile, findings: &mut Vec<Finding>) {
    let file_target = FindingTarget::File(wf.path.clone());
    if wf.concurrency.is_some() {
        findings.push(whatif_limit(
            file_target.clone(),
            "workflow concurrency is displayed, but queueing and cancellation are not simulated",
        ));
    }

    let pull_request = wf.triggers.pull_request.is_some();
    if pull_request && matches!(wf.permissions, Some(Permissions::WriteAll)) {
        findings.push(permissions_write_all(file_target));
    }

    for (job_id, job) in &wf.jobs {
        let target = FindingTarget::Job {
            file: wf.path.clone(),
            job: job_id.clone(),
        };
        if job.concurrency.is_some() {
            findings.push(whatif_limit(
                target.clone(),
                "job concurrency is displayed, but queueing and cancellation are not simulated",
            ));
        }
        if !job.services.is_empty() {
            findings.push(whatif_limit(
                target.clone(),
                "service containers are displayed, but containers are not started",
            ));
        }
        if job.environment.is_some() {
            findings.push(whatif_limit(
                target.clone(),
                "environment metadata is displayed, but protection rules are not evaluated",
            ));
        }
        if pull_request && matches!(job.permissions, Some(Permissions::WriteAll)) {
            findings.push(permissions_write_all(target));
        }
    }
}

/// Run `eval::evaluate` for `wf` under `ctx`, splitting its
/// `"<job>"` / `"<job>#<step_idx>"`-keyed result map into the
/// `AnalysisView` job/step state maps (keyed by file path as well, since
/// job ids alone aren't unique across files) and appending any findings
/// raised.
fn insert_run_states(
    wf: &WorkflowFile,
    ctx: &EvalContext,
    job_states: &mut BTreeMap<(PathBuf, String), RunState>,
    step_states: &mut BTreeMap<(PathBuf, String, usize), RunState>,
    findings: &mut Vec<Finding>,
) {
    if wf
        .triggers
        .push_branch_filter_mismatch(&ctx.event_name, &ctx.ref_name)
    {
        findings.push(trigger_filter(
            FindingTarget::File(wf.path.clone()),
            ctx.ref_name.clone(),
        ));
    }

    let (states, eval_findings) = evaluate(wf, ctx);
    findings.extend(eval_findings);

    for (key, state) in states {
        match key.split_once('#') {
            Some((job_id, step_idx)) => {
                let idx: usize = step_idx
                    .parse()
                    .expect("eval::evaluate's step key suffix is always a valid usize");
                step_states.insert((wf.path.clone(), job_id.to_string(), idx), state);
            }
            None => {
                job_states.insert((wf.path.clone(), key), state);
            }
        }
    }
}
