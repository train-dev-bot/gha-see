use gha_see::discover::discover_workflows;
use std::path::PathBuf;

#[test]
fn discovers_yaml_in_workflows_dir() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    // Place happy.yml directly in fixtures for this test; discover treats a dir of yml as workflow set
    let files = discover_workflows(&root).expect("discover");
    assert!(files.iter().any(|p| p.ends_with("happy.yml")));
}

#[test]
fn discovers_single_file() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/happy.yml");
    let files = discover_workflows(&file).expect("discover");
    assert_eq!(files.len(), 1);
}
