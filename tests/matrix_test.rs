use std::path::PathBuf;

use gha_see::analysis::analyze_path;
use gha_see::eval::{evaluate, EvalContext, RunState};
use gha_see::graph::build_job_graph;
use gha_see::ir::{normalize, SupportTier};
use gha_see::parse::parse_workflow_str;

#[test]
fn two_axis_matrix_expands_into_four_instances() {
    let raw = parse_workflow_str(include_str!("fixtures/matrix_2axis.yml")).unwrap();
    let wf = normalize(PathBuf::from("matrix_2axis.yml"), raw);

    assert_eq!(wf.instances.len(), 4);
    for id in [
        "test (node=18, os=ubuntu-latest)",
        "test (node=18, os=windows-latest)",
        "test (node=20, os=ubuntu-latest)",
        "test (node=20, os=windows-latest)",
    ] {
        assert!(wf.instances.contains_key(id), "missing instance {id}");
        assert_eq!(wf.instances[id].support, SupportTier::Supported);
        assert_eq!(wf.instances[id].base_id, "test");
    }
}

#[test]
fn matrix_condition_skips_non_matching_instances_only() {
    let raw = parse_workflow_str(include_str!("fixtures/matrix_2axis.yml")).unwrap();
    let wf = normalize(PathBuf::from("matrix_2axis.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(
        states["test (node=18, os=ubuntu-latest)"],
        RunState::WillRun
    );
    assert_eq!(
        states["test (node=18, os=windows-latest)"],
        RunState::WillRun
    );
    assert_eq!(
        states["test (node=20, os=ubuntu-latest)"],
        RunState::Skipped
    );
    assert_eq!(
        states["test (node=20, os=windows-latest)"],
        RunState::Skipped
    );
}

#[test]
fn include_merges_into_matching_combos_and_appends_new_one() {
    let raw = parse_workflow_str(include_str!("fixtures/matrix_include.yml")).unwrap();
    let wf = normalize(PathBuf::from("matrix_include.yml"), raw);

    assert_eq!(wf.instances.len(), 3);
    assert!(wf.instances.contains_key("test (color=green, fruit=apple)"));
    assert!(wf.instances.contains_key("test (color=green, fruit=pear)"));
    assert!(wf
        .instances
        .contains_key("test (color=yellow, fruit=banana)"));
}

#[test]
fn exclude_drops_matching_combo() {
    let raw = parse_workflow_str(include_str!("fixtures/matrix_exclude.yml")).unwrap();
    let wf = normalize(PathBuf::from("matrix_exclude.yml"), raw);

    assert_eq!(wf.instances.len(), 3);
    assert!(!wf
        .instances
        .contains_key("test (node=18, os=windows-latest)"));
    assert!(wf
        .instances
        .contains_key("test (node=20, os=windows-latest)"));
    assert!(wf
        .instances
        .contains_key("test (node=18, os=ubuntu-latest)"));
    assert!(wf
        .instances
        .contains_key("test (node=20, os=ubuntu-latest)"));
}

#[test]
fn empty_after_exclude_falls_back_to_single_deferred_instance_with_finding() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/matrix_empty.yml");
    let view = analyze_path(&file).unwrap();

    assert!(view.findings.iter().any(|f| f.code == "GHA_MATRIX_EMPTY"));
    assert_eq!(
        view.job_states[&(file, "test".to_string())],
        RunState::Deferred
    );
}

#[test]
fn oversize_matrix_is_capped_at_256_with_finding() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/matrix_cap.yml");
    let view = analyze_path(&file).unwrap();

    let cap_finding = view
        .findings
        .iter()
        .find(|f| f.code == "GHA_MATRIX_CAP")
        .expect("oversize matrix should raise GHA_MATRIX_CAP");
    assert!(cap_finding.message.contains("320"));

    let instance_count = view
        .workflows
        .iter()
        .find(|wf| wf.path == file)
        .unwrap()
        .instances
        .len();
    assert_eq!(instance_count, 256);
}

#[test]
fn dynamic_matrix_is_unsupported_and_stays_deferred() {
    let file =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/matrix_unsupported.yml");
    let view = analyze_path(&file).unwrap();

    use gha_see::findings::Severity;
    let finding = view
        .findings
        .iter()
        .find(|f| f.code == "GHA_MATRIX_UNSUPPORTED")
        .expect("dynamic matrix should raise GHA_MATRIX_UNSUPPORTED");
    assert_eq!(finding.severity, Severity::Info);

    assert_eq!(
        view.job_states[&(file, "test".to_string())],
        RunState::Deferred
    );
}

#[test]
fn needs_on_matrix_job_fans_out_to_every_instance() {
    let raw = parse_workflow_str(include_str!("fixtures/matrix_needs.yml")).unwrap();
    let wf = normalize(PathBuf::from("matrix_needs.yml"), raw);
    let (graph, findings) = build_job_graph(&wf);

    assert!(findings.is_empty());
    // 2 build instances -> 1 deploy instance = 2 edges.
    assert_eq!(graph.graph.edge_count(), 2);
    assert_eq!(graph.nodes.len(), 3);
    assert!(graph.nodes.contains_key("build (node=18)"));
    assert!(graph.nodes.contains_key("build (node=20)"));
    assert!(graph.nodes.contains_key("deploy"));
}

#[test]
fn deploy_waits_on_all_matrix_instances_of_build() {
    let raw = parse_workflow_str(include_str!("fixtures/matrix_needs.yml")).unwrap();
    let wf = normalize(PathBuf::from("matrix_needs.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build (node=18)"], RunState::WillRun);
    assert_eq!(states["build (node=20)"], RunState::WillRun);
    assert_eq!(states["deploy"], RunState::WillRun);
}

#[test]
fn jobs_without_strategy_get_a_single_instance_matching_the_base_id() {
    let raw = parse_workflow_str(include_str!("fixtures/happy.yml")).unwrap();
    let wf = normalize(PathBuf::from("happy.yml"), raw);

    assert_eq!(wf.instances.len(), 1);
    let instance = &wf.instances["build"];
    assert_eq!(instance.instance_id, "build");
    assert_eq!(instance.base_id, "build");
    assert!(instance.matrix.is_empty());
}
