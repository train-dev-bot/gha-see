use std::path::PathBuf;

use gha_see::analysis::analyze_path;
use gha_see::eval::RunState;
use gha_see::findings::Severity;
use gha_see::ir::normalize;
use gha_see::parse::parse_workflow_file;
use gha_see::uses::resolve_workflow_with_cache;
use petgraph::visit::EdgeRef;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uses")
}

fn caller_file() -> PathBuf {
    fixture_root().join(".github/workflows/caller.yml")
}

#[test]
fn resolves_local_composite_action_as_step_metadata() {
    let caller = caller_file();
    let view = analyze_path(&caller).unwrap();
    let workflow = view.workflows.iter().find(|wf| wf.path == caller).unwrap();
    let step = &workflow.jobs["build"].steps[0];

    assert_eq!(
        step.resolved_action_name.as_deref(),
        Some("Greeting action")
    );
    assert_eq!(step.composite_steps.len(), 2);
    assert_eq!(step.composite_steps[0].name.as_deref(), Some("Say hello"));
    assert!(!view
        .findings
        .iter()
        .any(|finding| finding.code == "GHA_DEFERRED"
            && finding.message.contains("./.github/actions/greet")));
}

#[test]
fn resolves_reusable_workflow_into_prefixed_instances_and_inputs() {
    let caller = caller_file();
    let view = analyze_path(&caller).unwrap();
    let workflow = view.workflows.iter().find(|wf| wf.path == caller).unwrap();

    assert!(!workflow.instances.contains_key("call"));
    assert!(workflow.instances.contains_key("call>prepare"));
    assert!(workflow.instances.contains_key("call>deploy"));
    assert_eq!(
        workflow.instances["call>deploy"].needs,
        vec!["call>prepare"]
    );
    assert_eq!(
        workflow.instances["call>deploy"]
            .inputs
            .get("target")
            .map(String::as_str),
        Some("staging")
    );
    assert_eq!(
        view.job_states[&(caller.clone(), "call>deploy".to_string())],
        RunState::WillRun
    );

    let graph_index = view
        .workflows
        .iter()
        .position(|wf| wf.path == caller)
        .unwrap();
    let graph = &view.graphs[graph_index];
    let prepare = graph.nodes["call>prepare"];
    let deploy = graph.nodes["call>deploy"];
    assert!(graph
        .graph
        .edges(prepare)
        .any(|edge| edge.target() == deploy));
    assert_eq!(workflow.instances["after"].needs, vec!["call>deploy"]);
}

#[test]
fn reports_remote_and_missing_uses_with_coaching() {
    let root = fixture_root();
    let view = analyze_path(&root).unwrap();

    let remote: Vec<_> = view
        .findings
        .iter()
        .filter(|finding| finding.code == "GHA_USES_REMOTE")
        .collect();
    // octo-org/...@main is never cached in CI; actions/checkout@v4 may already
    // be resolved from the developer cache (cache-aware analyze_path).
    assert!(
        remote
            .iter()
            .any(|finding| finding.message.contains("octo-org/ci")),
        "uncached remote reusable workflow should emit GHA_USES_REMOTE"
    );
    assert!(!remote.is_empty() && remote.len() <= 2);
    assert!(remote
        .iter()
        .all(|finding| finding.severity == Severity::Info && !finding.coach.is_empty()));

    let missing: Vec<_> = view
        .findings
        .iter()
        .filter(|finding| finding.code == "GHA_USES_MISSING")
        .collect();
    assert_eq!(missing.len(), 2);
    assert!(missing
        .iter()
        .all(|finding| finding.severity == Severity::Error && !finding.coach.is_empty()));
}

#[test]
fn reports_reusable_workflow_call_cycles() {
    let file = fixture_root().join(".github/workflows/cycle-a.yml");
    let view = analyze_path(&file).unwrap();

    let finding = view
        .findings
        .iter()
        .find(|finding| finding.code == "GHA_WORKFLOW_CALL")
        .expect("local reusable include cycle should be reported");
    assert_eq!(finding.severity, Severity::Error);
    assert!(!finding.coach.is_empty());
}

#[test]
fn expands_cached_remote_composites_and_reusable_workflows_recursively() {
    let root =
        std::env::temp_dir().join(format!("gha_see_remote_uses_test_{}", std::process::id()));
    let cache = root.join("cache");
    let workflow_file = root.join(".github/workflows/main.yml");
    std::fs::create_dir_all(workflow_file.parent().unwrap()).unwrap();
    std::fs::write(
        &workflow_file,
        r#"
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: acme/actions/setup@v1
      - uses: acme/node/tool@v1
  call:
    needs: build
    uses: acme/workflows/.github/workflows/reusable.yml@v2
  after:
    needs: call
    runs-on: ubuntu-latest
    steps:
      - run: echo done
"#,
    )
    .unwrap();
    write_cache_file(
        &cache,
        "acme/actions/v1/setup/action.yml",
        r#"
name: Remote setup
runs:
  using: composite
  steps:
    - uses: acme/nested/hello@v3
"#,
    );
    write_cache_file(
        &cache,
        "acme/nested/v3/hello/action.yml",
        r#"
name: Nested hello
runs:
  using: composite
  steps:
    - run: echo nested
"#,
    );
    write_cache_file(
        &cache,
        "acme/workflows/v2/.github/workflows/reusable.yml",
        r#"
on: workflow_call
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: acme/nested/hello@v3
"#,
    );
    write_cache_file(
        &cache,
        "acme/node/v1/tool/action.yml",
        r#"
name: Node metadata
inputs:
  mode:
    default: safe
runs:
  using: node20
  main: index.js
"#,
    );

    let raw = parse_workflow_file(&workflow_file).unwrap();
    let mut workflow = normalize(workflow_file.clone(), raw);
    let findings = resolve_workflow_with_cache(&mut workflow, &root, &cache);

    let setup = &workflow.jobs["build"].steps[0];
    assert_eq!(setup.resolved_action_name.as_deref(), Some("Remote setup"));
    assert_eq!(
        setup.composite_steps[0].resolved_action_name.as_deref(),
        Some("Nested hello")
    );
    assert_eq!(
        setup.composite_steps[0].composite_steps[0].run.as_deref(),
        Some("echo nested")
    );
    let node = &workflow.jobs["build"].steps[1];
    assert_eq!(node.resolved_action_name.as_deref(), Some("Node metadata"));
    assert_eq!(node.with_inputs["mode"], "safe");
    assert_eq!(node.action_runner.as_deref(), Some("node20"));
    assert_eq!(node.support, gha_see::ir::SupportTier::Supported);
    assert!(node.deferred_reasons.is_empty());
    assert!(workflow.instances.contains_key("call>deploy"));
    assert_eq!(workflow.instances["after"].needs, vec!["call>deploy"]);
    assert!(workflow.instances["call>deploy"].steps[0]
        .resolved_action_name
        .is_some());
    assert!(!findings
        .iter()
        .any(|finding| finding.code == "GHA_USES_REMOTE"));

    let _ = std::fs::remove_dir_all(root);
}

fn write_cache_file(cache: &std::path::Path, relative: &str, contents: &str) {
    let path = cache.join("gh").join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}
