//! Filesystem listing DTOs and helpers for the local web API.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsListResponse {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FsEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Error)]
pub enum FsListError {
    #[error("path not found: {}", .0.display())]
    NotFound(std::path::PathBuf),
    #[error("path is not a directory: {}", .0.display())]
    NotDirectory(std::path::PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Resolve to an absolute path so relative roots like `samples/` get a real
/// parent directory for Up navigation (`""` is not a valid list target).
fn resolve_directory(path: &Path) -> Result<PathBuf, FsListError> {
    let candidate = if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    };
    match fs::canonicalize(&candidate) {
        Ok(resolved) => Ok(resolved),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(FsListError::NotFound(candidate))
        }
        Err(error) => Err(FsListError::Io(error)),
    }
}

/// List direct children of an existing directory, with folders first and
/// workflow YAML files distinguished from ordinary files.
pub fn list_directory(path: &Path) -> Result<FsListResponse, FsListError> {
    let path = resolve_directory(path)?;
    if !path.is_dir() {
        return Err(FsListError::NotDirectory(path));
    }

    let mut entries = fs::read_dir(&path)?
        .map(|entry| {
            let entry = entry?;
            let entry_path = entry.path();
            let kind = if entry_path.is_dir() {
                "dir"
            } else if is_workflow(&entry_path) {
                "workflow"
            } else {
                "file"
            };
            Ok(FsEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry_path.display().to_string(),
                kind: kind.to_string(),
            })
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    entries.sort_by(|left, right| {
        kind_rank(&left.kind)
            .cmp(&kind_rank(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });

    let parent = path.parent().and_then(|parent| {
        // Skip empty parents (can appear for odd relative inputs before resolve).
        if parent.as_os_str().is_empty() {
            None
        } else {
            Some(parent.display().to_string())
        }
    });

    Ok(FsListResponse {
        path: path.display().to_string(),
        parent,
        entries,
    })
}

fn is_workflow(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("yml") | Some("yaml")
    )
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "dir" => 0,
        "workflow" => 1,
        _ => 2,
    }
}
