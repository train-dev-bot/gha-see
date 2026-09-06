use gha_see::parse::parse_workflow_str;

#[test]
fn parses_happy_job_and_steps() {
    let yaml = include_str!("fixtures/happy.yml");
    let wf = parse_workflow_str(yaml).expect("parse");
    assert_eq!(wf.name.as_deref(), Some("Happy"));
    assert!(wf.jobs.contains_key("build"));
    assert_eq!(wf.jobs["build"].steps.len(), 1);
}

#[test]
fn needs_accepts_string_or_list() {
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
    let wf = parse_workflow_str(yaml).unwrap();
    assert_eq!(wf.jobs["b"].needs, vec!["a".to_string()]);
    assert_eq!(wf.jobs["c"].needs, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn invalid_yaml_errors() {
    let err = parse_workflow_str("jobs: [\n:").unwrap_err();
    let _ = format!("{err}");
}
