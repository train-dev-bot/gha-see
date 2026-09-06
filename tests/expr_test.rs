use gha_see::expr::{extract_expressions, scan_workflow, unbalanced_expression};
use gha_see::ir::normalize;
use gha_see::parse::parse_workflow_str;
use std::path::PathBuf;

#[test]
fn traces_needs_outputs_binding() {
    let raw = parse_workflow_str(include_str!("fixtures/outputs_trace.yml")).unwrap();
    let wf = normalize(PathBuf::from("outputs_trace.yml"), raw);
    let (bindings, findings) = scan_workflow(&wf);

    assert!(findings.is_empty());
    assert!(bindings.iter().any(|b| b.producer_job == "build"
        && b.output_name == "artifact-id"
        && b.consumer_job == "test"));
}

#[test]
fn flags_unbalanced_expression() {
    assert!(gha_see::expr::unbalanced_expression("echo ${{ github.sha"));
}

#[test]
fn balanced_expression_is_not_flagged() {
    assert!(!unbalanced_expression("echo ${{ github.sha }}"));
}

#[test]
fn stray_closing_delimiter_is_unbalanced() {
    assert!(unbalanced_expression("echo }} github.sha"));
}

#[test]
fn extract_expressions_returns_trimmed_inner_contents() {
    let exprs = extract_expressions("echo ${{ needs.build.outputs.artifact-id }} done");
    assert_eq!(exprs, vec!["needs.build.outputs.artifact-id".to_string()]);
}

#[test]
fn extract_expressions_finds_multiple_expressions_in_order() {
    let exprs = extract_expressions("${{ a.b }} and ${{ c.d }}");
    assert_eq!(exprs, vec!["a.b".to_string(), "c.d".to_string()]);
}

#[test]
fn extract_expressions_skips_unmatched_open() {
    let exprs = extract_expressions("${{ a.b }} and ${{ trailing");
    assert_eq!(exprs, vec!["a.b".to_string()]);
}

#[test]
fn scan_workflow_flags_syntax_error_in_step_run() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo ${{ github.sha
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("bad.yml"), raw);
    let (bindings, findings) = scan_workflow(&wf);

    assert!(bindings.is_empty());
    assert!(findings.iter().any(|f| f.code == "GHA_EXPR_SYNTAX"));
}

#[test]
fn scan_workflow_finds_binding_in_job_if_condition() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    outputs:
      ok: "true"
    steps:
      - run: echo build
  deploy:
    runs-on: ubuntu-latest
    needs: [build]
    if: ${{ needs.build.outputs.ok == 'true' }}
    steps:
      - run: echo deploy
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (bindings, findings) = scan_workflow(&wf);

    assert!(findings.is_empty());
    assert!(bindings.iter().any(|b| b.consumer_job == "deploy"
        && b.producer_job == "build"
        && b.output_name == "ok"
        && b.raw == "needs.build.outputs.ok"));
}

#[test]
fn scan_workflow_ignores_expressions_without_needs_outputs() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo ${{ github.sha }}
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (bindings, findings) = scan_workflow(&wf);

    assert!(bindings.is_empty());
    assert!(findings.is_empty());
}

#[test]
fn scan_workflow_finds_bindings_in_step_with_and_env_and_strategy() {
    let yaml = r#"
jobs:
  build:
    runs-on: ubuntu-latest
    outputs:
      artifact: value
      region: value
      lane: value
    steps:
      - run: echo build
  deploy:
    needs: build
    runs-on: ubuntu-latest
    strategy:
      matrix:
        lane: ["${{ needs.build.outputs.lane }}"]
    steps:
      - uses: example/action@v1
        with:
          artifact: "${{ needs.build.outputs.artifact }}"
        env:
          REGION: "${{ needs.build.outputs.region }}"
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("bindings.yml"), raw);

    let (bindings, findings) = scan_workflow(&wf);

    assert!(findings.is_empty());
    for output in ["artifact", "region", "lane"] {
        assert!(
            bindings
                .iter()
                .any(|binding| binding.consumer_job.starts_with("deploy")
                    && binding.producer_job == "build"
                    && binding.output_name == output),
            "missing binding for {output}"
        );
    }
}
