use std::path::PathBuf;

use gha_see::analysis::analyze_path;
use gha_see::api::{EvalContextDto, WebView};
use gha_see::eval::EvalContext;

#[test]
fn dataflow_sample_webview_has_workflows_and_edges_or_bindings() {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples/03_dataflow_outputs.yml");
    let view = analyze_path(&sample).expect("dataflow sample should analyze");
    let ctx = EvalContext::default_mock_for(&view.workflows);
    let web = WebView::from_analysis(&view, &sample, &ctx);

    assert!(
        !web.workflows.is_empty(),
        "WebView workflows should be non-empty for dataflow sample"
    );

    let has_edges = web.graphs.iter().any(|g| !g.edges.is_empty());
    let has_bindings = !web.bindings.is_empty();
    assert!(
        has_edges || has_bindings,
        "dataflow sample WebView should have graph edges or bindings; edges={} bindings={}",
        web.graphs.iter().map(|g| g.edges.len()).sum::<usize>(),
        web.bindings.len()
    );
}

#[test]
fn eval_context_dto_serde_json_roundtrip() {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples/03_dataflow_outputs.yml");
    let view = analyze_path(&sample).expect("dataflow sample should analyze");
    let ctx = EvalContext::default_mock_for(&view.workflows);
    let dto = EvalContextDto::from(&ctx);

    let json = serde_json::to_string(&dto).expect("serialize EvalContextDto");
    let back: EvalContextDto = serde_json::from_str(&json).expect("deserialize EvalContextDto");

    let json2 = serde_json::to_string(&back).expect("re-serialize EvalContextDto");
    assert_eq!(
        json, json2,
        "EvalContextDto serde_json roundtrip should be stable"
    );
}

#[test]
fn dispatch_inputs_are_scoped_to_each_workflow_file() {
    let samples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples");
    let view = analyze_path(&samples).expect("samples dir should analyze");
    let ctx = EvalContext::default_mock_for(&view.workflows);
    let web = WebView::from_analysis(&view, &samples, &ctx);

    let typescript = web
        .workflows
        .iter()
        .find(|w| w.path.ends_with("11_cicd_typescript.yml"))
        .expect("typescript showcase");
    let ts_names: Vec<&str> = typescript
        .dispatch_inputs
        .iter()
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(ts_names, vec!["environment", "force_rollback", "skip_dast"]);
    assert!(
        !ts_names.contains(&"dry_run"),
        "typescript workflow must not inherit dispatch inputs from other files"
    );

    let dispatch = web
        .workflows
        .iter()
        .find(|w| w.path.ends_with("07_dispatch_inputs.yml"))
        .expect("dispatch inputs sample");
    let dispatch_names: Vec<&str> = dispatch
        .dispatch_inputs
        .iter()
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        dispatch_names,
        vec!["dry_run", "environment", "release_name"]
    );

    let happy = web
        .workflows
        .iter()
        .find(|w| w.path.ends_with("01_happy_path.yml"))
        .expect("happy path");
    assert!(
        happy.dispatch_inputs.is_empty(),
        "workflows without dispatch/call inputs should expose an empty list"
    );
}
