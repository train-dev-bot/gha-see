//! Structured parsing for the common GitHub Actions `on:` trigger shapes.

use std::collections::BTreeMap;

use serde_yaml::{Mapping, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerSet {
    pub push: Option<PushTrigger>,
    pub pull_request: Option<PullRequestTrigger>,
    pub schedules: Vec<ScheduleTrigger>,
    pub workflow_dispatch: Option<WorkflowDispatchTrigger>,
    pub workflow_call: Option<WorkflowCallTrigger>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushTrigger {
    pub branches: Vec<String>,
    pub branches_ignore: Vec<String>,
    pub tags: Vec<String>,
    pub tags_ignore: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullRequestTrigger {
    pub types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleTrigger {
    pub cron: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowDispatchTrigger {
    pub inputs: BTreeMap<String, TriggerInput>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowCallTrigger {
    pub inputs: BTreeMap<String, TriggerInput>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerInput {
    pub description: Option<String>,
    pub required: bool,
    pub default: Option<String>,
    pub input_type: Option<String>,
    pub options: Vec<String>,
}

impl TriggerSet {
    pub fn parse(value: &Value) -> Self {
        let mut triggers = Self::default();
        match value {
            Value::String(event) => triggers.enable(event, &Value::Null),
            Value::Sequence(events) => {
                for event in events {
                    if let Some(event) = event.as_str() {
                        triggers.enable(event, &Value::Null);
                    }
                }
            }
            Value::Mapping(events) => {
                for (event, config) in events {
                    if let Some(event) = event.as_str() {
                        triggers.enable(event, config);
                    }
                }
            }
            _ => {}
        }
        triggers
    }

    pub fn dispatch_defaults(&self) -> BTreeMap<String, String> {
        self.workflow_dispatch
            .as_ref()
            .map(|dispatch| {
                dispatch
                    .inputs
                    .iter()
                    .filter_map(|(name, input)| {
                        input.default.clone().map(|value| (name.clone(), value))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return `true` only when a push branch allow/ignore filter excludes
    /// the supplied mock ref. Tags and unfiltered push triggers are left
    /// alone because this helper intentionally models only branch filtering.
    pub fn push_branch_filter_mismatch(&self, event_name: &str, git_ref: &str) -> bool {
        if event_name != "push" {
            return false;
        }
        let Some(push) = &self.push else {
            return false;
        };
        let Some(branch) = git_ref.strip_prefix("refs/heads/") else {
            return false;
        };
        if push.branches.is_empty() && push.branches_ignore.is_empty() {
            return false;
        }

        let included = push.branches.is_empty()
            || push.branches.iter().any(|pattern| matches(pattern, branch));
        let ignored = push
            .branches_ignore
            .iter()
            .any(|pattern| matches(pattern, branch));
        !included || ignored
    }

    fn enable(&mut self, event: &str, config: &Value) {
        match event {
            "push" => self.push = Some(parse_push(config)),
            "pull_request" => self.pull_request = Some(parse_pull_request(config)),
            "schedule" => self.schedules = parse_schedules(config),
            "workflow_dispatch" => {
                self.workflow_dispatch = Some(WorkflowDispatchTrigger {
                    inputs: parse_inputs(config),
                });
            }
            "workflow_call" => {
                self.workflow_call = Some(WorkflowCallTrigger {
                    inputs: parse_inputs(config),
                });
            }
            _ => {}
        }
    }
}

fn parse_push(value: &Value) -> PushTrigger {
    let Some(map) = value.as_mapping() else {
        return PushTrigger::default();
    };
    PushTrigger {
        branches: string_list(map, "branches"),
        branches_ignore: string_list(map, "branches-ignore"),
        tags: string_list(map, "tags"),
        tags_ignore: string_list(map, "tags-ignore"),
    }
}

fn parse_pull_request(value: &Value) -> PullRequestTrigger {
    PullRequestTrigger {
        types: value
            .as_mapping()
            .map(|map| string_list(map, "types"))
            .unwrap_or_default(),
    }
}

fn parse_schedules(value: &Value) -> Vec<ScheduleTrigger> {
    value
        .as_sequence()
        .into_iter()
        .flatten()
        .filter_map(Value::as_mapping)
        .filter_map(|schedule| {
            scalar_string(mapping_get(schedule, "cron")?).map(|cron| ScheduleTrigger { cron })
        })
        .collect()
}

fn parse_inputs(value: &Value) -> BTreeMap<String, TriggerInput> {
    let Some(inputs) = value
        .as_mapping()
        .and_then(|config| mapping_get(config, "inputs"))
        .and_then(Value::as_mapping)
    else {
        return BTreeMap::new();
    };

    inputs
        .iter()
        .filter_map(|(name, schema)| {
            let name = name.as_str()?.to_string();
            let schema = schema.as_mapping();
            let input = TriggerInput {
                description: schema
                    .and_then(|map| mapping_get(map, "description"))
                    .and_then(scalar_string),
                required: schema
                    .and_then(|map| mapping_get(map, "required"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                default: schema
                    .and_then(|map| mapping_get(map, "default"))
                    .and_then(scalar_string),
                input_type: schema
                    .and_then(|map| mapping_get(map, "type"))
                    .and_then(scalar_string),
                options: schema
                    .map(|map| string_list(map, "options"))
                    .unwrap_or_default(),
            };
            Some((name, input))
        })
        .collect()
}

fn string_list(map: &Mapping, key: &str) -> Vec<String> {
    match mapping_get(map, key) {
        Some(Value::Sequence(values)) => values.iter().filter_map(scalar_string).collect(),
        Some(value) => scalar_string(value).into_iter().collect(),
        None => Vec::new(),
    }
}

fn mapping_get<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn matches(pattern: &str, branch: &str) -> bool {
    glob::Pattern::new(pattern)
        .map(|pattern| pattern.matches(branch))
        .unwrap_or_else(|_| pattern == branch)
}
