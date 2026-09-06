//! Opt-in fetching and cache layout for remote `uses:` references.

use std::fmt;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use thiserror::Error;

/// A GitHub-hosted action or reusable-workflow reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemoteRef {
    pub owner: String,
    pub repo: String,
    pub subpath: Option<String>,
    pub rev: String,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRemoteRefError {
    uses: String,
}

impl fmt::Display for ParseRemoteRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "`{}` is not an owner/repository remote uses reference",
            self.uses
        )
    }
}

impl std::error::Error for ParseRemoteRefError {}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("could not prepare remote uses cache: {0}")]
    Cache(#[from] std::io::Error),
}

/// Result of ensuring one remote repository revision exists in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    Cached { remote: RemoteRef, path: PathBuf },
    Fetched { remote: RemoteRef, path: PathBuf },
    Failed { remote: RemoteRef, error: String },
}

impl RemoteRef {
    pub fn parse(uses: &str) -> Result<Self, ParseRemoteRefError> {
        let invalid = || ParseRemoteRefError {
            uses: uses.to_string(),
        };
        if uses.starts_with("./") || uses.starts_with(".github/") || uses.starts_with("docker://") {
            return Err(invalid());
        }

        let (location, rev) = uses.rsplit_once('@').unwrap_or((uses, "HEAD"));
        if location.is_empty() || rev.is_empty() {
            return Err(invalid());
        }
        let mut segments = location.split('/');
        let owner = segments.next().filter(|part| valid_segment(part));
        let repo = segments.next().filter(|part| valid_segment(part));
        let (Some(owner), Some(repo)) = (owner, repo) else {
            return Err(invalid());
        };
        let remainder = segments.collect::<Vec<_>>();
        if remainder
            .iter()
            .any(|part| !valid_segment(part) || *part == "." || *part == "..")
        {
            return Err(invalid());
        }

        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
            subpath: (!remainder.is_empty()).then(|| remainder.join("/")),
            rev: rev.to_string(),
            raw: uses.to_string(),
        })
    }

    /// Repository root for this revision. A `subpath`, when present, is
    /// resolved beneath this root by the uses resolver.
    pub fn cache_path(&self, cache_root: &Path) -> PathBuf {
        cache_root
            .join("gh")
            .join(&self.owner)
            .join(&self.repo)
            .join(sanitize_component(&self.rev))
    }

    pub fn target_path(&self, cache_root: &Path) -> PathBuf {
        let root = self.cache_path(cache_root);
        self.subpath
            .as_ref()
            .map_or(root.clone(), |subpath| root.join(subpath))
    }
}

pub fn default_cache_root() -> Option<PathBuf> {
    dirs::cache_dir().map(|path| path.join("gha-see"))
}

/// Fetch a GitHub repository archive unless this revision is already
/// complete in the cache.
pub fn ensure_cached(remote: &RemoteRef, cache_root: &Path) -> Result<FetchOutcome, FetchError> {
    ensure_cached_with(remote, cache_root, download_archive)
}

/// Downloader-injected form used by deterministic, offline tests.
pub fn ensure_cached_with<F, E>(
    remote: &RemoteRef,
    cache_root: &Path,
    download: F,
) -> Result<FetchOutcome, FetchError>
where
    F: FnOnce(&RemoteRef) -> Result<Vec<u8>, E>,
    E: fmt::Display,
{
    let destination = remote.cache_path(cache_root);
    if destination.join(".gha-see-complete").is_file() {
        return Ok(FetchOutcome::Cached {
            remote: remote.clone(),
            path: destination,
        });
    }

    let Some(parent) = destination.parent() else {
        return Ok(FetchOutcome::Failed {
            remote: remote.clone(),
            error: "cache destination has no parent directory".to_string(),
        });
    };
    fs::create_dir_all(parent)?;

    let bytes = match download(remote) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(FetchOutcome::Failed {
                remote: remote.clone(),
                error: error.to_string(),
            });
        }
    };

    match unpack_repository_archive(&bytes, &destination) {
        Ok(()) => Ok(FetchOutcome::Fetched {
            remote: remote.clone(),
            path: destination,
        }),
        Err(error) => Ok(FetchOutcome::Failed {
            remote: remote.clone(),
            error: format!("invalid repository archive: {error}"),
        }),
    }
}

fn download_archive(remote: &RemoteRef) -> Result<Vec<u8>, String> {
    let url = archive_url(remote)?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("gha-see/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.get(url);
    if let Some(token) = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
        .or_else(|| {
            std::env::var("GH_TOKEN")
                .ok()
                .filter(|token| !token.is_empty())
        })
    {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| error.to_string())?;
    response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|error| error.to_string())
}

fn archive_url(remote: &RemoteRef) -> Result<reqwest::Url, String> {
    let mut url =
        reqwest::Url::parse("https://codeload.github.com").map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "invalid codeload base URL".to_string())?
        .extend([
            remote.owner.as_str(),
            remote.repo.as_str(),
            "tar.gz",
            remote.rev.as_str(),
        ]);
    Ok(url)
}

fn unpack_repository_archive(bytes: &[u8], destination: &Path) -> Result<(), std::io::Error> {
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "cache path has no parent")
    })?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("revision");
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    if temporary.exists() {
        fs::remove_dir_all(&temporary)?;
    }
    fs::create_dir_all(&temporary)?;

    let result = (|| {
        let decoder = GzDecoder::new(Cursor::new(bytes));
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&temporary)?;

        let mut roots = fs::read_dir(&temporary)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        if roots.len() != 1 || !roots[0].is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected one repository root directory",
            ));
        }
        let extracted = roots.pop().expect("length checked above");
        if destination.exists() {
            fs::remove_dir_all(destination)?;
        }
        fs::rename(extracted, destination)?;
        fs::write(destination.join(".gha-see-complete"), b"")?;
        Ok(())
    })();

    let _ = fs::remove_dir_all(&temporary);
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
