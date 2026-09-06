//! Offline resolution for local composite actions and reusable workflows.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::fetch::RemoteRef;
use crate::findings::{uses_missing, uses_remote, workflow_call_cycle, Finding, FindingTarget};
use crate::ir::{normalize, JobInstance, Step, SupportTier, WorkflowFile};
use crate::parse::{parse_workflow_file, RawStep};

#[derive(Debug, Deserialize)]
struct ActionFile {
    name: Option<String>,
    #[serde(default)]
    inputs: BTreeMap<String, ActionInput>,
    runs: ActionRuns,
}

#[derive(Debug, Deserialize)]
struct ActionInput {
    default: Option<serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
struct ActionRuns {
    using: String,
    #[serde(default)]
    steps: Vec<RawStep>,
}

#[derive(Clone)]
struct ReusableCall {
    id: String,
    uses: String,
    inputs: BTreeMap<String, String>,
    needs: Vec<String>,
}

const MAX_USES_DEPTH: usize = 8;

#[derive(Clone, Copy)]
struct ResolveConfig<'a> {
    cache_root: Option<&'a Path>,
}

/// Resolve all `uses:` references in one normalized workflow. This is a cold
/// pipeline operation: it reads local YAML, but evaluation/re-evaluation does
/// not.
pub fn resolve_workflow(wf: &mut WorkflowFile, repo_root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut stack = vec![canonical_or_owned(&wf.path)];
    resolve_internal(
        wf,
        repo_root,
        ResolveConfig { cache_root: None },
        0,
        &mut stack,
        &mut findings,
    );
    findings
}

/// Resolve local uses plus remote references whose repositories were
/// explicitly fetched into `cache_root`. This function never opens the
/// network.
pub fn resolve_workflow_with_cache(
    wf: &mut WorkflowFile,
    repo_root: &Path,
    cache_root: &Path,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut stack = vec![canonical_or_owned(&wf.path)];
    resolve_internal(
        wf,
        repo_root,
        ResolveConfig {
            cache_root: Some(cache_root),
        },
        0,
        &mut stack,
        &mut findings,
    );
    findings
}

fn resolve_internal(
    wf: &mut WorkflowFile,
    repo_root: &Path,
    config: ResolveConfig<'_>,
    depth: usize,
    stack: &mut Vec<PathBuf>,
    findings: &mut Vec<Finding>,
) {
    resolve_steps(wf, repo_root, config, depth, findings);

    let calls: Vec<ReusableCall> = wf
        .jobs
        .values()
        .filter_map(|job| {
            job.uses.as_ref().map(|uses| ReusableCall {
                id: job.id.clone(),
                uses: uses.clone(),
                inputs: job.with_inputs.clone(),
                needs: job.needs.clone(),
            })
        })
        .collect();

    for call in calls {
        let target = FindingTarget::Job {
            file: wf.path.clone(),
            job: call.id.clone(),
        };
        if depth >= MAX_USES_DEPTH {
            findings.push(uses_remote(target, call.uses));
            defer_call(wf, &call.id);
            continue;
        }

        let Some((path, callee_root)) = workflow_target(repo_root, &call.uses, config) else {
            if is_local(&call.uses) {
                findings.push(uses_missing(target, call.uses));
            } else {
                findings.push(uses_remote(target, call.uses));
            }
            defer_call(wf, &call.id);
            continue;
        };
        let canonical = canonical_or_owned(&path);
        if let Some(cycle_start) = stack.iter().position(|entry| entry == &canonical) {
            let mut chain = stack[cycle_start..].to_vec();
            chain.push(canonical);
            findings.push(workflow_call_cycle(wf.path.clone(), &chain));
            defer_call(wf, &call.id);
            continue;
        }

        let Ok(raw) = parse_workflow_file(&path) else {
            if is_local(&call.uses) {
                findings.push(uses_missing(target, call.uses));
            } else {
                findings.push(uses_remote(target, call.uses));
            }
            defer_call(wf, &call.id);
            continue;
        };
        let mut callee = normalize(path, raw);
        stack.push(canonical);
        resolve_internal(
            &mut callee,
            &callee_root,
            config,
            depth + 1,
            stack,
            findings,
        );
        stack.pop();

        inline_callee(wf, &call, callee.instances);
    }
}

fn resolve_steps(
    wf: &mut WorkflowFile,
    repo_root: &Path,
    config: ResolveConfig<'_>,
    depth: usize,
    findings: &mut Vec<Finding>,
) {
    for (job_id, job) in &mut wf.jobs {
        for (step_idx, step) in job.steps.iter_mut().enumerate() {
            let target = FindingTarget::Step {
                file: wf.path.clone(),
                job: job_id.clone(),
                step_idx,
            };
            resolve_action_step(
                step,
                repo_root,
                config,
                depth,
                &target,
                &mut BTreeSet::new(),
                findings,
            );
        }
    }

    // Matrix expansion happens before uses resolution, so refresh each
    // already-expanded instance with the resolved logical step metadata.
    for instance in wf.instances.values_mut() {
        if let Some(job) = wf.jobs.get(&instance.base_id) {
            instance.steps = job.steps.clone();
        }
    }
}

fn resolve_action_step(
    step: &mut Step,
    repo_root: &Path,
    config: ResolveConfig<'_>,
    depth: usize,
    target: &FindingTarget,
    stack: &mut BTreeSet<PathBuf>,
    findings: &mut Vec<Finding>,
) {
    let Some(uses) = step.uses.clone() else {
        return;
    };
    if depth >= MAX_USES_DEPTH {
        step.support = SupportTier::Deferred;
        step.deferred_reasons
            .push(format!("uses expansion exceeded depth {MAX_USES_DEPTH}"));
        return;
    }

    let Some((action_path, action_root)) = action_target(repo_root, &uses, config) else {
        if is_local(&uses) {
            findings.push(uses_missing(target.clone(), uses));
        } else {
            findings.push(uses_remote(target.clone(), uses));
        }
        step.support = SupportTier::Deferred;
        return;
    };
    let canonical = canonical_or_owned(&action_path);
    if !stack.insert(canonical.clone()) {
        step.support = SupportTier::Deferred;
        step.deferred_reasons
            .push("composite action uses cycle".to_string());
        return;
    }

    let parsed = fs::read_to_string(&action_path)
        .ok()
        .and_then(|contents| serde_yaml::from_str::<ActionFile>(&contents).ok());
    let Some(action) = parsed else {
        if is_local(&uses) {
            findings.push(uses_missing(target.clone(), uses));
        } else {
            findings.push(uses_remote(target.clone(), uses));
        }
        step.support = SupportTier::Deferred;
        stack.remove(&canonical);
        return;
    };
    let mut inputs = action
        .inputs
        .into_iter()
        .filter_map(|(name, input)| input.default.map(|value| (name, stringify(value))))
        .collect::<BTreeMap<_, _>>();
    inputs.extend(step.with_inputs.clone());
    step.with_inputs = inputs;
    step.resolved_action_name = action.name;

    // Node/docker/etc. actions are fully *readable* (name, inputs, runner) but
    // gha-see never executes them. Treat successful metadata resolve as
    // Supported so the UI is not flooded with GHA_DEFERRED on every
    // actions/checkout@v4-style step. Only composites get inlined steps.
    step.action_runner = Some(action.runs.using.clone());
    if !action.runs.using.eq_ignore_ascii_case("composite") {
        step.support = SupportTier::Supported;
        step.deferred_reasons.clear();
        stack.remove(&canonical);
        return;
    }

    step.composite_steps = action
        .runs
        .steps
        .into_iter()
        .map(normalize_action_step)
        .collect();
    for child in &mut step.composite_steps {
        resolve_action_step(
            child,
            &action_root,
            config,
            depth + 1,
            target,
            stack,
            findings,
        );
    }
    step.support = SupportTier::Supported;
    step.deferred_reasons.clear();
    stack.remove(&canonical);
}

fn inline_callee(
    caller: &mut WorkflowFile,
    call: &ReusableCall,
    callee_instances: BTreeMap<String, JobInstance>,
) {
    caller
        .instances
        .retain(|_, instance| instance.base_id != call.id);

    let mut nested = BTreeMap::new();
    for (_, mut instance) in callee_instances {
        instance.instance_id = prefixed(&call.id, &instance.instance_id);
        instance.base_id = prefixed(&call.id, &instance.base_id);
        instance.needs = instance
            .needs
            .into_iter()
            .map(|need| prefixed(&call.id, &need))
            .collect();
        if instance.needs.is_empty() {
            instance.needs = call.needs.clone();
        }
        let mut inputs = call.inputs.clone();
        inputs.extend(instance.inputs);
        instance.inputs = inputs;
        nested.insert(instance.instance_id.clone(), instance);
    }

    let referenced: BTreeSet<String> = nested
        .values()
        .flat_map(|instance| instance.needs.iter().cloned())
        .collect();
    let terminal_bases: Vec<String> = nested
        .values()
        .map(|instance| instance.base_id.clone())
        .filter(|base| !referenced.contains(base))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    for instance in caller.instances.values_mut() {
        if instance.needs.iter().any(|need| need == &call.id) {
            instance.needs = instance
                .needs
                .iter()
                .flat_map(|need| {
                    if need == &call.id {
                        terminal_bases.clone()
                    } else {
                        vec![need.clone()]
                    }
                })
                .collect();
        }
    }
    caller.instances.extend(nested);
}

fn defer_call(wf: &mut WorkflowFile, call_id: &str) {
    if let Some(job) = wf.jobs.get_mut(call_id) {
        job.support = SupportTier::Deferred;
    }
    for instance in wf
        .instances
        .values_mut()
        .filter(|instance| instance.base_id == call_id)
    {
        instance.support = SupportTier::Deferred;
    }
}

fn normalize_action_step(raw: RawStep) -> Step {
    Step {
        name: raw.name,
        id: raw.id,
        uses: raw.uses,
        run: raw.run,
        shell: raw.shell,
        working_directory: raw.working_directory,
        condition: raw.condition,
        with_inputs: raw
            .with_inputs
            .into_iter()
            .map(|(key, value)| (key, stringify(value)))
            .collect(),
        env: raw.env.into_iter().collect(),
        resolved_action_name: None,
        action_runner: None,
        composite_steps: Vec::new(),
        support: SupportTier::Supported,
        deferred_reasons: Vec::new(),
    }
}

fn is_local(uses: &str) -> bool {
    uses.starts_with("./") || uses.starts_with(".github/")
}

fn workflow_target(
    repo_root: &Path,
    uses: &str,
    config: ResolveConfig<'_>,
) -> Option<(PathBuf, PathBuf)> {
    if is_local(uses) {
        return local_target(repo_root, uses)
            .filter(|path| path.is_file())
            .map(|path| (path, repo_root.to_path_buf()));
    }
    let cache_root = config.cache_root?;
    let remote = RemoteRef::parse(uses).ok()?;
    let path = remote.target_path(cache_root);
    path.is_file()
        .then(|| (path, remote.cache_path(cache_root)))
}

fn action_target(
    repo_root: &Path,
    uses: &str,
    config: ResolveConfig<'_>,
) -> Option<(PathBuf, PathBuf)> {
    if is_local(uses) {
        return local_action_file(repo_root, uses).map(|path| (path, repo_root.to_path_buf()));
    }
    let cache_root = config.cache_root?;
    let remote = RemoteRef::parse(uses).ok()?;
    let path = action_file_at(remote.target_path(cache_root))?;
    Some((path, remote.cache_path(cache_root)))
}

fn local_target(repo_root: &Path, uses: &str) -> Option<PathBuf> {
    let relative = uses.strip_prefix("./").unwrap_or(uses);
    let root = repo_root.canonicalize().ok()?;
    let target = root.join(relative).canonicalize().ok()?;
    target.starts_with(&root).then_some(target)
}

fn local_action_file(repo_root: &Path, uses: &str) -> Option<PathBuf> {
    let target = local_target(repo_root, uses)?;
    action_file_at(target)
}

fn action_file_at(target: PathBuf) -> Option<PathBuf> {
    if target.is_file() {
        let is_action = target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "action.yml" | "action.yaml"));
        return is_action.then_some(target);
    }
    ["action.yml", "action.yaml"]
        .into_iter()
        .map(|name| target.join(name))
        .find(|path| path.is_file())
}

fn prefixed(prefix: &str, id: &str) -> String {
    format!("{prefix}>{id}")
}

fn canonical_or_owned(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn stringify(value: serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Null => "null".to_string(),
        serde_yaml::Value::Bool(value) => value.to_string(),
        serde_yaml::Value::Number(value) => value.to_string(),
        serde_yaml::Value::String(value) => value,
        other => serde_yaml::to_string(&other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}
