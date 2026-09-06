use gha_see::graph::{build_job_graph, topo_levels};
use gha_see::ir::normalize;
use gha_see::parse::parse_workflow_str;
use std::path::PathBuf;

#[test]
fn detects_cycle() {
    let raw = parse_workflow_str(include_str!("fixtures/cycle.yml")).unwrap();
    let wf = normalize(PathBuf::from("cycle.yml"), raw);
    let (graph, findings) = build_job_graph(&wf);

    assert!(findings.iter().any(|f| f.code == "GHA_CYCLE"));
    // The graph is still built even though it's cyclic.
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.nodes.contains_key("a"));
    assert!(graph.nodes.contains_key("b"));
}

#[test]
fn detects_missing_need() {
    let raw = parse_workflow_str(include_str!("fixtures/bad_needs.yml")).unwrap();
    let wf = normalize(PathBuf::from("bad_needs.yml"), raw);
    let (graph, findings) = build_job_graph(&wf);

    assert!(findings.iter().any(|f| f.code == "GHA_MISSING_NEED"));
    // The dangling `needs:` shouldn't produce a phantom node.
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.nodes.contains_key("build"));
}

#[test]
fn happy_graph_has_no_findings() {
    let yaml = r#"
name: N
on: push
jobs:
  a:
    runs-on: ubuntu-latest
    steps: [{run: echo a}]
  b:
    runs-on: ubuntu-latest
    needs: a
    steps: [{run: echo b}]
  c:
    runs-on: ubuntu-latest
    needs: [a, b]
    steps: [{run: echo c}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (graph, findings) = build_job_graph(&wf);

    assert!(findings.is_empty());
    assert_eq!(graph.graph.edge_count(), 3);
}

#[test]
fn topo_levels_orders_roots_before_dependents() {
    let yaml = r#"
name: N
on: push
jobs:
  a:
    runs-on: ubuntu-latest
    steps: [{run: echo a}]
  b:
    runs-on: ubuntu-latest
    needs: a
    steps: [{run: echo b}]
  c:
    runs-on: ubuntu-latest
    needs: [a, b]
    steps: [{run: echo c}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (graph, _findings) = build_job_graph(&wf);
    let levels = topo_levels(&graph);

    assert_eq!(
        levels,
        vec![
            vec!["a".to_string()],
            vec!["b".to_string()],
            vec!["c".to_string()],
        ]
    );
}

#[test]
fn topo_levels_groups_independent_roots_together() {
    let yaml = r#"
name: N
on: push
jobs:
  a:
    runs-on: ubuntu-latest
    steps: [{run: echo a}]
  b:
    runs-on: ubuntu-latest
    steps: [{run: echo b}]
  c:
    runs-on: ubuntu-latest
    needs: [a, b]
    steps: [{run: echo c}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (graph, _findings) = build_job_graph(&wf);
    let levels = topo_levels(&graph);

    assert_eq!(
        levels,
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string()],
        ]
    );
}

#[test]
fn topo_levels_omits_cyclic_jobs() {
    let raw = parse_workflow_str(include_str!("fixtures/cycle.yml")).unwrap();
    let wf = normalize(PathBuf::from("cycle.yml"), raw);
    let (graph, _findings) = build_job_graph(&wf);
    let levels = topo_levels(&graph);

    // Neither `a` nor `b` ever reaches in-degree 0, so no level is produced.
    assert!(levels.is_empty());
}
