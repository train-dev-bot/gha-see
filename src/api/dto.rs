//! CamelCase JSON DTOs for the web UI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};

use crate::analysis::{workflow_has_remote_uses, workflow_needs_remote_fetch, AnalysisView};
use crate::eval::{EvalContext, GithubCtx, NeedStatus, RunState};
use crate::findings::{Finding, FindingTarget, Severity};
use crate::graph::JobGraph;
use crate::ir::{JobInstance, Step, SupportTier, WorkflowFile};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebView {
    pub path: String,
    pub workflows: Vec<WorkflowDto>,
    pub graphs: Vec<GraphDto>,
    pub bindings: Vec<BindingDto>,
    pub findings: Vec<FindingDto>,
    pub job_states: Vec<JobStateDto>,
    pub step_states: Vec<StepStateDto>,
    pub context: EvalContextDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDto {
    pub index: usize,
    pub path: String,
    pub name: Option<String>,
    pub parse_ok: bool,
    pub file_error: Option<String>,
    pub raw_source: Option<String>,
    pub has_remote_uses: bool,
    /// True when remote `uses:` still need an explicit Fetch (cache miss).
    pub needs_remote_fetch: bool,
    pub instances: Vec<InstanceDto>,
    /// `workflow_dispatch` / `workflow_call` inputs declared on this file.
    pub dispatch_inputs: Vec<TriggerInputDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDto {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencyDto {
    pub group: String,
    pub cancel_in_progress: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDto {
    pub name: String,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerInputDto {
    pub name: String,
    pub default: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDto {
    pub instance_id: String,
    pub base_id: String,
    pub matrix: BTreeMap<String, String>,
    pub runs_on: Option<String>,
    pub needs: Vec<String>,
    pub condition: Option<String>,
    pub outputs: BTreeMap<String, String>,
    pub steps: Vec<StepDto>,
    pub support: SupportTierDto,
    pub environment: Option<EnvironmentDto>,
    pub concurrency: Option<ConcurrencyDto>,
    pub services: Vec<ServiceDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepDto {
    pub index: usize,
    pub name: Option<String>,
    pub id: Option<String>,
    pub uses: Option<String>,
    pub run: Option<String>,
    pub shell: Option<String>,
    pub working_directory: Option<String>,
    pub condition: Option<String>,
    pub with_inputs: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub resolved_action_name: Option<String>,
    pub action_runner: Option<String>,
    pub composite_step_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDto {
    pub workflow_index: usize,
    pub nodes: Vec<String>,
    pub edges: Vec<[String; 2]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingDto {
    pub file: String,
    pub consumer_job: String,
    pub consumer_step_idx: Option<usize>,
    pub producer_job: String,
    pub output_name: String,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingDto {
    pub severity: SeverityDto,
    pub code: String,
    pub message: String,
    pub coach: String,
    pub target: FindingTargetDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStateDto {
    pub file: String,
    pub instance_id: String,
    pub state: RunStateDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStateDto {
    pub file: String,
    pub instance_id: String,
    pub step_idx: usize,
    pub state: RunStateDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalContextDto {
    pub event_name: String,
    pub ref_name: String,
    pub github: GithubDto,
    pub env: BTreeMap<String, String>,
    pub vars: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, String>,
    pub secrets: Vec<String>,
    pub needs: BTreeMap<String, NeedStatusDto>,
    pub repo_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDto {
    pub sha: String,
    pub repository: String,
    pub actor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedStatusDto {
    pub result: String,
    pub outputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunStateDto {
    WillRun,
    Skipped,
    Unknown,
    Deferred,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SupportTierDto {
    Supported,
    Deferred,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SeverityDto {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FindingTargetDto {
    File {
        file: String,
    },
    Job {
        file: String,
        job: String,
    },
    Step {
        file: String,
        job: String,
        step_idx: usize,
    },
}

impl WebView {
    pub fn from_analysis(view: &AnalysisView, root: &Path, ctx: &EvalContext) -> Self {
        let workflows = view
            .workflows
            .iter()
            .enumerate()
            .map(|(index, wf)| workflow_dto(index, wf))
            .collect();

        let graphs = view
            .graphs
            .iter()
            .enumerate()
            .map(|(workflow_index, g)| graph_dto(workflow_index, g))
            .collect();

        let bindings = view
            .bindings
            .iter()
            .map(|(file, b)| BindingDto {
                file: path_str(file),
                consumer_job: b.consumer_job.clone(),
                consumer_step_idx: b.consumer_step_idx,
                producer_job: b.producer_job.clone(),
                output_name: b.output_name.clone(),
                raw: b.raw.clone(),
            })
            .collect();

        let findings = view.findings.iter().map(finding_dto).collect();

        let job_states = view
            .job_states
            .iter()
            .map(|((file, instance_id), state)| JobStateDto {
                file: path_str(file),
                instance_id: instance_id.clone(),
                state: (*state).into(),
            })
            .collect();

        let step_states = view
            .step_states
            .iter()
            .map(|((file, instance_id, step_idx), state)| StepStateDto {
                file: path_str(file),
                instance_id: instance_id.clone(),
                step_idx: *step_idx,
                state: (*state).into(),
            })
            .collect();

        WebView {
            path: path_str(root),
            workflows,
            graphs,
            bindings,
            findings,
            job_states,
            step_states,
            context: EvalContextDto::from(ctx),
        }
    }
}

fn workflow_dto(index: usize, wf: &WorkflowFile) -> WorkflowDto {
    let mut instances: Vec<InstanceDto> = wf.instances.values().map(instance_dto).collect();
    instances.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));

    WorkflowDto {
        index,
        path: path_str(&wf.path),
        name: wf.name.clone(),
        parse_ok: wf.parse_ok,
        file_error: wf.file_error.clone(),
        raw_source: wf.raw_source.clone(),
        has_remote_uses: workflow_has_remote_uses(wf),
        needs_remote_fetch: workflow_needs_remote_fetch(wf),
        instances,
        dispatch_inputs: dispatch_input_dtos(wf),
    }
}

fn dispatch_input_dtos(wf: &WorkflowFile) -> Vec<TriggerInputDto> {
    let mut by_name = BTreeMap::new();
    if let Some(call) = &wf.triggers.workflow_call {
        for (name, input) in &call.inputs {
            by_name.insert(name.clone(), input.clone());
        }
    }
    if let Some(dispatch) = &wf.triggers.workflow_dispatch {
        for (name, input) in &dispatch.inputs {
            by_name.insert(name.clone(), input.clone());
        }
    }
    by_name
        .into_iter()
        .map(|(name, input)| TriggerInputDto {
            name,
            default: input.default,
            required: input.required,
        })
        .collect()
}

fn instance_dto(inst: &JobInstance) -> InstanceDto {
    InstanceDto {
        instance_id: inst.instance_id.clone(),
        base_id: inst.base_id.clone(),
        matrix: inst.matrix.clone(),
        runs_on: inst.runs_on.clone(),
        needs: inst.needs.clone(),
        condition: inst.condition.clone(),
        outputs: inst.outputs.clone(),
        steps: inst
            .steps
            .iter()
            .enumerate()
            .map(|(index, s)| step_dto(index, s))
            .collect(),
        support: inst.support.into(),
        environment: inst.environment.as_ref().map(|e| EnvironmentDto {
            name: e.name.clone(),
            url: e.url.clone(),
        }),
        concurrency: inst.concurrency.as_ref().map(|c| ConcurrencyDto {
            group: c.group.clone(),
            cancel_in_progress: c.cancel_in_progress,
        }),
        services: inst
            .services
            .iter()
            .map(|(name, svc)| ServiceDto {
                name: name.clone(),
                image: svc.image.clone(),
            })
            .collect(),
    }
}

fn step_dto(index: usize, step: &Step) -> StepDto {
    StepDto {
        index,
        name: step.name.clone(),
        id: step.id.clone(),
        uses: step.uses.clone(),
        run: step.run.clone(),
        shell: step.shell.clone(),
        working_directory: step.working_directory.clone(),
        condition: step.condition.clone(),
        with_inputs: step.with_inputs.clone(),
        env: step.env.clone(),
        resolved_action_name: step.resolved_action_name.clone(),
        action_runner: step.action_runner.clone(),
        composite_step_count: step.composite_steps.len(),
    }
}

fn graph_dto(workflow_index: usize, graph: &JobGraph) -> GraphDto {
    let mut nodes: Vec<String> = graph.nodes.keys().cloned().collect();
    nodes.sort();

    let mut edges = Vec::new();
    for edge in graph.graph.edge_references() {
        let from = graph.graph[edge.source()].clone();
        let to = graph.graph[edge.target()].clone();
        edges.push([from, to]);
    }
    edges.sort();

    GraphDto {
        workflow_index,
        nodes,
        edges,
    }
}

fn finding_dto(f: &Finding) -> FindingDto {
    FindingDto {
        severity: f.severity.into(),
        code: f.code.to_string(),
        message: f.message.clone(),
        coach: f.coach.clone(),
        target: finding_target_dto(&f.target),
    }
}

fn finding_target_dto(t: &FindingTarget) -> FindingTargetDto {
    match t {
        FindingTarget::File(file) => FindingTargetDto::File {
            file: path_str(file),
        },
        FindingTarget::Job { file, job } => FindingTargetDto::Job {
            file: path_str(file),
            job: job.clone(),
        },
        FindingTarget::Step {
            file,
            job,
            step_idx,
        } => FindingTargetDto::Step {
            file: path_str(file),
            job: job.clone(),
            step_idx: *step_idx,
        },
    }
}

fn path_str(path: &Path) -> String {
    path.display().to_string()
}

impl From<&EvalContext> for EvalContextDto {
    fn from(ctx: &EvalContext) -> Self {
        EvalContextDto {
            event_name: ctx.event_name.clone(),
            ref_name: ctx.ref_name.clone(),
            github: GithubDto {
                sha: ctx.github.sha.clone(),
                repository: ctx.github.repository.clone(),
                actor: ctx.github.actor.clone(),
            },
            env: ctx.env.clone(),
            vars: ctx.vars.clone(),
            inputs: ctx.inputs.clone(),
            secrets: ctx.secrets.iter().cloned().collect(),
            needs: ctx
                .needs
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        NeedStatusDto {
                            result: v.result.clone(),
                            outputs: v.outputs.clone(),
                        },
                    )
                })
                .collect(),
            repo_root: ctx.repo_root.as_ref().map(|p| path_str(p)),
        }
    }
}

impl From<EvalContextDto> for EvalContext {
    fn from(dto: EvalContextDto) -> Self {
        EvalContext {
            event_name: dto.event_name,
            ref_name: dto.ref_name,
            github: GithubCtx {
                sha: dto.github.sha,
                repository: dto.github.repository,
                actor: dto.github.actor,
            },
            env: dto.env,
            vars: dto.vars,
            inputs: dto.inputs,
            secrets: dto.secrets.into_iter().collect::<BTreeSet<_>>(),
            needs: dto
                .needs
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        NeedStatus {
                            result: v.result,
                            outputs: v.outputs,
                        },
                    )
                })
                .collect(),
            repo_root: dto.repo_root.map(PathBuf::from),
        }
    }
}

impl From<RunState> for RunStateDto {
    fn from(s: RunState) -> Self {
        match s {
            RunState::WillRun => RunStateDto::WillRun,
            RunState::Skipped => RunStateDto::Skipped,
            RunState::Unknown => RunStateDto::Unknown,
            RunState::Deferred => RunStateDto::Deferred,
        }
    }
}

impl From<SupportTier> for SupportTierDto {
    fn from(s: SupportTier) -> Self {
        match s {
            SupportTier::Supported => SupportTierDto::Supported,
            SupportTier::Deferred => SupportTierDto::Deferred,
            SupportTier::Unknown => SupportTierDto::Unknown,
        }
    }
}

impl From<Severity> for SeverityDto {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Error => SeverityDto::Error,
            Severity::Warning => SeverityDto::Warning,
            Severity::Info => SeverityDto::Info,
        }
    }
}
