//! Normalize the tolerant `parse::Raw*` structs into a semantic intermediate
//! representation (IR), tagging each job/step with a [`SupportTier`] so
//! constructs not yet evaluated by v1 (e.g. `strategy.matrix`, reusable
//! workflow calls, non-literal `runs-on`) are surfaced explicitly instead of
//! silently mis-rendered.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::parse::{RawJob, RawStep, RawWorkflow};
use crate::triggers::TriggerSet;

/// How confidently this layer understands a job or step.
///
/// `Deferred` constructs are recognized-but-not-yet-evaluated (e.g. matrix
/// expansion); `Unknown` is reserved for constructs this layer cannot
/// recognize at all. v1 `normalize` only ever produces `Supported` or
/// `Deferred`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportTier {
    Supported,
    Deferred,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct WorkflowFile {
    pub path: PathBuf,
    /// Exact file contents retained for source inspection. Workflows built
    /// directly from parsed values may not have an originating source.
    pub raw_source: Option<String>,
    pub name: Option<String>,
    /// Original trigger YAML, retained for tolerant round-tripping of event
    /// shapes outside the common structured subset.
    pub on: serde_yaml::Value,
    pub triggers: TriggerSet,
    pub concurrency: Option<Concurrency>,
    pub permissions: Option<Permissions>,
    pub defaults_run: Option<DefaultsRun>,
    pub jobs: BTreeMap<String, Job>,
    /// Every job, expanded to its concrete run instances: jobs without a
    /// `strategy.matrix` get exactly one instance (`instance_id == job id`);
    /// jobs with an expandable matrix get one instance per combination.
    /// Graph/eval/TUI all key off this map rather than `jobs` so matrix
    /// fan-out is consistent everywhere. See `crate::matrix::expand_workflow`.
    pub instances: BTreeMap<String, JobInstance>,
    pub parse_ok: bool,
    pub file_error: Option<String>,
}

impl WorkflowFile {
    /// The logical (YAML) job id that `instance_id` was expanded from, if
    /// `instance_id` names a known instance.
    pub fn base_id_of(&self, instance_id: &str) -> Option<&str> {
        self.instances.get(instance_id).map(|i| i.base_id.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Concurrency {
    pub group: String,
    pub cancel_in_progress: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub image: Option<String>,
    pub ports: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub options: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permissions {
    ReadAll,
    WriteAll,
    Map(BTreeMap<String, String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultsRun {
    pub shell: Option<String>,
    pub working_directory: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub uses: Option<String>,
    pub with_inputs: BTreeMap<String, String>,
    pub runs_on: Option<String>,
    pub needs: Vec<String>,
    pub condition: Option<String>,
    pub outputs: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub concurrency: Option<Concurrency>,
    pub services: BTreeMap<String, Service>,
    pub permissions: Option<Permissions>,
    pub environment: Option<Environment>,
    pub defaults_run: Option<DefaultsRun>,
    pub steps: Vec<Step>,
    /// The raw `strategy:` mapping, kept verbatim from the parsed YAML so
    /// `crate::matrix::expand_workflow` can expand `strategy.matrix` after
    /// normalization. Presence alone no longer marks a job `Deferred` — see
    /// `matrix_note`.
    pub strategy: Option<serde_yaml::Value>,
    /// Set by `crate::matrix::expand_workflow` when this job's
    /// `strategy.matrix` needed a `GHA_MATRIX_*` finding (empty product,
    /// capped, or unsupported shape). `None` means either no matrix or a
    /// matrix that expanded cleanly with no cap.
    pub matrix_note: Option<crate::matrix::MatrixNote>,
    pub support: SupportTier,
    pub deferred_reasons: Vec<String>,
}

/// A single concrete run of a [`Job`]: either the job's only instance (no
/// `strategy.matrix`, `instance_id == base_id`) or one `strategy.matrix`
/// combination (`instance_id` includes the matrix values, e.g.
/// `"test (node=18)"`).
///
/// Instances clone the fields they need from their `Job` rather than
/// borrowing, so graph/eval/TUI can treat `WorkflowFile::instances` as the
/// single source of truth without also threading `jobs` through everywhere.
#[derive(Debug, Clone)]
pub struct JobInstance {
    pub instance_id: String,
    pub base_id: String,
    pub matrix: BTreeMap<String, String>,
    /// Reusable-workflow inputs available as `inputs.*` while evaluating
    /// this nested job.
    pub inputs: BTreeMap<String, String>,
    pub runs_on: Option<String>,
    pub needs: Vec<String>,
    pub condition: Option<String>,
    pub outputs: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub strategy: Option<serde_yaml::Value>,
    pub concurrency: Option<Concurrency>,
    pub services: BTreeMap<String, Service>,
    pub permissions: Option<Permissions>,
    pub environment: Option<Environment>,
    pub defaults_run: Option<DefaultsRun>,
    pub steps: Vec<Step>,
    pub support: SupportTier,
    pub deferred_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub name: Option<String>,
    pub id: Option<String>,
    pub uses: Option<String>,
    pub run: Option<String>,
    pub shell: Option<String>,
    pub working_directory: Option<String>,
    pub condition: Option<String>,
    pub with_inputs: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    /// Resolved metadata for a local/remote action (`action.yml` name).
    pub resolved_action_name: Option<String>,
    /// `runs.using` from action.yml when resolved (e.g. `composite`, `node20`).
    pub action_runner: Option<String>,
    pub composite_steps: Vec<Step>,
    pub support: SupportTier,
    pub deferred_reasons: Vec<String>,
}

/// Normalize a successfully-parsed workflow into the IR.
///
/// This function cannot fail: unsupported constructs are tagged with
/// `SupportTier::Deferred` (plus a human-readable reason) rather than
/// rejected, so a single unsupported job never prevents the rest of the
/// workflow from being rendered.
pub fn normalize(path: PathBuf, raw: RawWorkflow) -> WorkflowFile {
    let triggers = TriggerSet::parse(&raw.on);
    let concurrency = raw.concurrency.as_ref().and_then(normalize_concurrency);
    let permissions = raw.permissions.as_ref().and_then(normalize_permissions);
    let defaults_run = raw.defaults.as_ref().and_then(normalize_defaults_run);
    let mut jobs: BTreeMap<String, Job> = raw
        .jobs
        .into_iter()
        .map(|(id, job)| (id.clone(), normalize_job(id, job, defaults_run.as_ref())))
        .collect();

    let instances = crate::matrix::expand_workflow(&mut jobs);

    WorkflowFile {
        path,
        raw_source: None,
        name: raw.name,
        on: raw.on,
        triggers,
        concurrency,
        permissions,
        defaults_run,
        jobs,
        instances,
        parse_ok: true,
        file_error: None,
    }
}

fn normalize_job(id: String, raw: RawJob, workflow_defaults: Option<&DefaultsRun>) -> Job {
    let mut deferred_reasons = Vec::new();
    let remote_uses = raw
        .uses
        .as_deref()
        .is_some_and(|uses| !uses.starts_with("./") && !uses.starts_with(".github/"));

    let runs_on = normalize_runs_on(raw.runs_on, &mut deferred_reasons);
    let concurrency = raw.concurrency.as_ref().and_then(normalize_concurrency);
    let services = normalize_services(raw.services);
    let permissions = raw.permissions.as_ref().and_then(normalize_permissions);
    let environment = raw.environment.as_ref().and_then(normalize_environment);
    let defaults_run = raw.defaults.as_ref().and_then(normalize_defaults_run);
    let steps = raw
        .steps
        .into_iter()
        .map(|step| normalize_step(step, workflow_defaults, defaults_run.as_ref()))
        .collect();

    let support = if deferred_reasons.is_empty() && !remote_uses {
        SupportTier::Supported
    } else {
        SupportTier::Deferred
    };

    Job {
        id,
        uses: raw.uses,
        with_inputs: stringify_scalar_map(raw.with_inputs),
        runs_on,
        needs: raw.needs,
        condition: raw.condition,
        outputs: raw.outputs.into_iter().collect(),
        env: raw.env.into_iter().collect(),
        concurrency,
        services,
        permissions,
        environment,
        defaults_run,
        steps,
        strategy: raw.strategy,
        matrix_note: None, // filled in by `crate::matrix::expand_workflow`
        support,
        deferred_reasons,
    }
}

fn normalize_step(
    raw: RawStep,
    workflow_defaults: Option<&DefaultsRun>,
    job_defaults: Option<&DefaultsRun>,
) -> Step {
    let inherited_shell = job_defaults
        .and_then(|defaults| defaults.shell.clone())
        .or_else(|| workflow_defaults.and_then(|defaults| defaults.shell.clone()));
    let inherited_working_directory = job_defaults
        .and_then(|defaults| defaults.working_directory.clone())
        .or_else(|| workflow_defaults.and_then(|defaults| defaults.working_directory.clone()));
    Step {
        name: raw.name,
        id: raw.id,
        uses: raw.uses,
        run: raw.run,
        shell: raw.shell.or(inherited_shell),
        working_directory: raw.working_directory.or(inherited_working_directory),
        condition: raw.condition,
        with_inputs: stringify_scalar_map(raw.with_inputs),
        env: raw.env.into_iter().collect(),
        resolved_action_name: None,
        action_runner: None,
        composite_steps: Vec::new(),
        support: SupportTier::Supported,
        deferred_reasons: Vec::new(),
    }
}

fn normalize_concurrency(value: &serde_yaml::Value) -> Option<Concurrency> {
    match value {
        serde_yaml::Value::String(group) => Some(Concurrency {
            group: group.clone(),
            cancel_in_progress: None,
        }),
        serde_yaml::Value::Mapping(map) => {
            let group = map
                .get(serde_yaml::Value::String("group".to_string()))
                .and_then(value_to_string)?;
            let cancel_in_progress = map
                .get(serde_yaml::Value::String("cancel-in-progress".to_string()))
                .and_then(serde_yaml::Value::as_bool);
            Some(Concurrency {
                group,
                cancel_in_progress,
            })
        }
        _ => None,
    }
}

fn normalize_services(
    services: std::collections::HashMap<String, serde_yaml::Value>,
) -> BTreeMap<String, Service> {
    services
        .into_iter()
        .filter_map(|(name, value)| {
            let map = value.as_mapping()?;
            let image = map
                .get(serde_yaml::Value::String("image".to_string()))
                .and_then(value_to_string);
            let ports = map
                .get(serde_yaml::Value::String("ports".to_string()))
                .and_then(serde_yaml::Value::as_sequence)
                .map(|ports| ports.iter().filter_map(value_to_string).collect())
                .unwrap_or_default();
            let env = map
                .get(serde_yaml::Value::String("env".to_string()))
                .and_then(serde_yaml::Value::as_mapping)
                .map(stringify_mapping)
                .unwrap_or_default();
            let options = map
                .get(serde_yaml::Value::String("options".to_string()))
                .and_then(value_to_string);
            Some((
                name,
                Service {
                    image,
                    ports,
                    env,
                    options,
                },
            ))
        })
        .collect()
}

fn normalize_permissions(value: &serde_yaml::Value) -> Option<Permissions> {
    match value {
        serde_yaml::Value::String(value) if value == "read-all" => Some(Permissions::ReadAll),
        serde_yaml::Value::String(value) if value == "write-all" => Some(Permissions::WriteAll),
        serde_yaml::Value::Mapping(map) => Some(Permissions::Map(stringify_mapping(map))),
        _ => None,
    }
}

fn normalize_environment(value: &serde_yaml::Value) -> Option<Environment> {
    match value {
        serde_yaml::Value::String(name) => Some(Environment {
            name: name.clone(),
            url: None,
        }),
        serde_yaml::Value::Mapping(map) => Some(Environment {
            name: map
                .get(serde_yaml::Value::String("name".to_string()))
                .and_then(value_to_string)?,
            url: map
                .get(serde_yaml::Value::String("url".to_string()))
                .and_then(value_to_string),
        }),
        _ => None,
    }
}

fn normalize_defaults_run(value: &serde_yaml::Value) -> Option<DefaultsRun> {
    let run = value
        .as_mapping()?
        .get(serde_yaml::Value::String("run".to_string()))?
        .as_mapping()?;
    Some(DefaultsRun {
        shell: run
            .get(serde_yaml::Value::String("shell".to_string()))
            .and_then(value_to_string),
        working_directory: run
            .get(serde_yaml::Value::String("working-directory".to_string()))
            .and_then(value_to_string),
    })
}

fn stringify_mapping(map: &serde_yaml::Mapping) -> BTreeMap<String, String> {
    map.iter()
        .filter_map(|(key, value)| Some((key.as_str()?.to_string(), value_to_string(value)?)))
        .collect()
}

fn value_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::Null => Some("null".to_string()),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        serde_yaml::Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn stringify_scalar_map(
    values: std::collections::HashMap<String, serde_yaml::Value>,
) -> BTreeMap<String, String> {
    values
        .into_iter()
        .map(|(key, value)| {
            let value = match value {
                serde_yaml::Value::Null => "null".to_string(),
                serde_yaml::Value::Bool(value) => value.to_string(),
                serde_yaml::Value::Number(value) => value.to_string(),
                serde_yaml::Value::String(value) => value,
                other => serde_yaml::to_string(&other)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            };
            (key, value)
        })
        .collect()
}

/// Flatten `runs-on` to `Option<String>`. A plain YAML string passes through
/// unchanged. A sequence or mapping (self-hosted label lists, runner
/// groups, `${{ matrix.* }}` expressions, ...) is stringified best-effort
/// and always paired with a deferred-reason note, since v1 does not
/// evaluate expressions or resolve runner groups/labels.
fn normalize_runs_on(
    raw_runs_on: Option<serde_yaml::Value>,
    deferred_reasons: &mut Vec<String>,
) -> Option<String> {
    match raw_runs_on {
        None => None,
        Some(serde_yaml::Value::String(s)) => Some(s),
        Some(other) => {
            deferred_reasons.push(format!(
                "runs-on is a {} (not a plain string); matrix/expression runners not evaluated in v1",
                value_kind(&other)
            ));
            stringify_runs_on_best_effort(&other)
        }
    }
}

fn value_kind(value: &serde_yaml::Value) -> &'static str {
    match value {
        serde_yaml::Value::Null => "null",
        serde_yaml::Value::Bool(_) => "bool",
        serde_yaml::Value::Number(_) => "number",
        serde_yaml::Value::String(_) => "string",
        serde_yaml::Value::Sequence(_) => "sequence",
        serde_yaml::Value::Mapping(_) => "mapping",
        serde_yaml::Value::Tagged(_) => "tagged value",
    }
}

/// Best-effort stringification for non-string `runs-on` shapes:
/// - a sequence of string labels (`[self-hosted, linux]`) joins with `, `
/// - a runner-group mapping (`{group: ubuntu-runners}`) yields the group name
/// - anything else (expressions, unrecognized mappings) yields `None`
fn stringify_runs_on_best_effort(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::Sequence(seq) => {
            let labels: Vec<&str> = seq.iter().filter_map(|v| v.as_str()).collect();
            if labels.is_empty() || labels.len() != seq.len() {
                None
            } else {
                Some(labels.join(", "))
            }
        }
        serde_yaml::Value::Mapping(map) => map
            .get(serde_yaml::Value::String("group".to_string()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}
