use std::path::PathBuf;

#[test]
#[ignore]
fn corpus_does_not_panic() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    if !root.exists() {
        return;
    }
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) == Some("yml")
            || path.extension().and_then(|s| s.to_str()) == Some("yaml")
        {
            let _ = gha_see::analysis::analyze_path(&path);
        }
    }
}
