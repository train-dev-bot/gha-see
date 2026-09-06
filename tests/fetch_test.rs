use std::io::Write;
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;
use gha_see::fetch::{ensure_cached_with, FetchOutcome, RemoteRef};

#[test]
fn parses_remote_action_and_cache_layout() {
    let remote = RemoteRef::parse("actions/checkout@v4").unwrap();

    assert_eq!(remote.owner, "actions");
    assert_eq!(remote.repo, "checkout");
    assert_eq!(remote.subpath, None);
    assert_eq!(remote.rev, "v4");
    assert_eq!(
        remote.cache_path(Path::new("/tmp/cache")),
        Path::new("/tmp/cache/gh/actions/checkout/v4")
    );
}

#[test]
fn parses_remote_subpath_and_workflow_path() {
    let action = RemoteRef::parse("acme/tools/actions/setup@feature/test").unwrap();
    assert_eq!(action.subpath.as_deref(), Some("actions/setup"));
    assert_eq!(action.rev, "feature/test");
    assert!(action
        .cache_path(Path::new("/tmp/cache"))
        .ends_with("gh/acme/tools/feature_test"));

    let workflow = RemoteRef::parse("acme/platform/.github/workflows/deploy.yml@0123456").unwrap();
    assert_eq!(
        workflow.subpath.as_deref(),
        Some(".github/workflows/deploy.yml")
    );
}

#[test]
fn accepts_missing_ref_as_head_and_rejects_local_or_malformed_uses() {
    assert_eq!(RemoteRef::parse("acme/tools").unwrap().rev, "HEAD");
    assert!(RemoteRef::parse("./.github/actions/local").is_err());
    assert!(RemoteRef::parse("docker://alpine:latest").is_err());
    assert!(RemoteRef::parse("owner-only@v1").is_err());
}

#[test]
fn downloads_archive_once_and_reuses_complete_cache() {
    let cache =
        std::env::temp_dir().join(format!("gha_see_fetch_test_{}_cache", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let remote = RemoteRef::parse("acme/tools/actions/setup@v1").unwrap();
    let archive = repository_archive(&[(
        "tools-v1/actions/setup/action.yml",
        b"name: Setup\nruns:\n  using: composite\n  steps: []\n",
    )]);

    let first = ensure_cached_with(&remote, &cache, |_| Ok::<_, String>(archive)).unwrap();
    assert!(matches!(first, FetchOutcome::Fetched { .. }));
    assert!(remote
        .cache_path(&cache)
        .join(".gha-see-complete")
        .is_file());
    assert!(remote.target_path(&cache).join("action.yml").is_file());

    let second = ensure_cached_with(&remote, &cache, |_| -> Result<Vec<u8>, String> {
        panic!("a complete cache entry must not download again")
    })
    .unwrap();
    assert!(matches!(second, FetchOutcome::Cached { .. }));

    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn default_cache_root_is_named_gha_see() {
    let root = gha_see::fetch::default_cache_root().expect("platform cache dir");
    assert_eq!(root.file_name().unwrap(), "gha-see");
}

#[test]
fn failed_download_does_not_leave_a_complete_cache_entry() {
    let cache =
        std::env::temp_dir().join(format!("gha_see_fetch_test_{}_failure", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let remote = RemoteRef::parse("acme/missing@v1").unwrap();

    let outcome = ensure_cached_with(&remote, &cache, |_| Err::<Vec<u8>, _>("HTTP 404")).unwrap();
    assert!(matches!(
        outcome,
        FetchOutcome::Failed { ref error, .. } if error.contains("HTTP 404")
    ));
    assert!(!remote.cache_path(&cache).exists());

    let _ = std::fs::remove_dir_all(cache);
}

fn repository_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for (path, contents) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, path, &mut &contents[..])
            .unwrap();
    }
    let encoder = archive.into_inner().unwrap();
    let mut bytes = encoder.finish().unwrap();
    bytes.flush().unwrap();
    bytes
}
