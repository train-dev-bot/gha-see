use std::path::PathBuf;

use gha_see::analysis::{analyze_path, revaluate};
use gha_see::eval::{EvalContext, RunState};
use gha_see::ir::normalize;
use gha_see::parse::parse_workflow_str;

#[test]
fn normalizes_common_trigger_shapes() {
    let raw = parse_workflow_str(
        r#"
name: Triggers
on:
  push:
    branches: [main, "release/**"]
    branches-ignore: [wip]
    tags: ["v*"]
  pull_request:
    types: [opened, synchronize]
  schedule:
    - cron: "0 4 * * 1"
  workflow_dispatch:
    inputs:
      environment:
        description: Deploy target
        required: true
        default: staging
        type: choice
        options: [staging, production]
  workflow_call:
    inputs:
      dry_run:
        required: false
        default: true
        type: boolean
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    )
    .unwrap();

    let workflow = normalize(PathBuf::from("triggers.yml"), raw);
    let push = workflow.triggers.push.as_ref().unwrap();
    assert_eq!(push.branches, vec!["main", "release/**"]);
    assert_eq!(push.branches_ignore, vec!["wip"]);
    assert_eq!(push.tags, vec!["v*"]);
    assert_eq!(
        workflow.triggers.pull_request.as_ref().unwrap().types,
        vec!["opened", "synchronize"]
    );
    assert_eq!(workflow.triggers.schedules[0].cron, "0 4 * * 1");

    let dispatch = workflow.triggers.workflow_dispatch.as_ref().unwrap();
    let environment = &dispatch.inputs["environment"];
    assert_eq!(environment.description.as_deref(), Some("Deploy target"));
    assert!(environment.required);
    assert_eq!(environment.default.as_deref(), Some("staging"));
    assert_eq!(environment.input_type.as_deref(), Some("choice"));
    assert_eq!(environment.options, vec!["staging", "production"]);

    let call = workflow.triggers.workflow_call.as_ref().unwrap();
    assert_eq!(call.inputs["dry_run"].default.as_deref(), Some("true"));
    assert_eq!(
        call.inputs["dry_run"].input_type.as_deref(),
        Some("boolean")
    );
}

#[test]
fn scalar_and_sequence_on_values_enable_named_triggers() {
    let scalar = normalize(
        PathBuf::from("push.yml"),
        parse_workflow_str("on: push\njobs: {}\n").unwrap(),
    );
    assert!(scalar.triggers.push.is_some());

    let sequence = normalize(
        PathBuf::from("events.yml"),
        parse_workflow_str("on: [pull_request, workflow_dispatch]\njobs: {}\n").unwrap(),
    );
    assert!(sequence.triggers.pull_request.is_some());
    assert!(sequence.triggers.workflow_dispatch.is_some());
}

#[test]
fn analysis_seeds_dispatch_input_defaults() {
    let dir = std::env::temp_dir().join(format!(
        "gha_see_triggers_test_{}_dispatch_defaults",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dispatch.yml");
    std::fs::write(
        &file,
        r#"
on:
  workflow_dispatch:
    inputs:
      environment:
        default: staging
jobs:
  deploy:
    runs-on: ubuntu-latest
    if: inputs.environment == 'staging'
    steps:
      - run: echo deploy
"#,
    )
    .unwrap();

    let view = analyze_path(&file).unwrap();
    assert_eq!(
        view.job_states[&(file.clone(), "deploy".to_string())],
        RunState::WillRun
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn branch_filter_finding_tracks_the_mock_ref() {
    let dir = std::env::temp_dir().join(format!(
        "gha_see_triggers_test_{}_branch_filter",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("release.yml");
    std::fs::write(
        &file,
        r#"
on:
  push:
    branches: ["release/**"]
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: echo publish
"#,
    )
    .unwrap();

    let view = analyze_path(&file).unwrap();
    assert!(view
        .findings
        .iter()
        .any(|finding| finding.code == "GHA_TRIGGER_FILTER"));

    let mut release_ctx = EvalContext::default_mock();
    release_ctx.ref_name = "refs/heads/release/1.0".to_string();
    let release_view = revaluate(&view, &release_ctx);
    assert!(!release_view
        .findings
        .iter()
        .any(|finding| finding.code == "GHA_TRIGGER_FILTER"));

    let _ = std::fs::remove_dir_all(&dir);
}
