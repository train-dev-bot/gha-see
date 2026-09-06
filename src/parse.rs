//! Parse GitHub Actions workflow YAML into tolerant raw structs.
//!
//! This layer only knows about YAML shape, not GHA semantics: unknown keys are
//! preserved via `extra` rather than rejected (no `deny_unknown_fields`), so
//! later stages (`ir`) can decide whether an unrecognized/unsupported
//! construct should be `Deferred` or `Unknown` instead of failing the whole
//! file to parse.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::Deserialize;
use thiserror::Error;

/// A workflow's `jobs.<job_id>.needs` value, normalized to a list regardless of
/// whether the YAML author wrote a single string or a sequence of strings.
pub type Needs = Vec<String>;

#[derive(Debug, Clone, Deserialize)]
pub struct RawWorkflow {
    pub name: Option<String>,
    #[serde(default)]
    pub on: serde_yaml::Value,
    pub concurrency: Option<serde_yaml::Value>,
    pub permissions: Option<serde_yaml::Value>,
    pub defaults: Option<serde_yaml::Value>,
    #[serde(default)]
    pub jobs: HashMap<String, RawJob>,
    /// Anything else (`concurrency`, `permissions`, ...) is preserved here
    /// rather than dropped, so later layers (`analysis`) can surface
    /// workflow-level constructs like `concurrency` as deferred findings.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawJob {
    #[serde(rename = "runs-on")]
    pub runs_on: Option<serde_yaml::Value>,
    #[serde(default, deserialize_with = "deserialize_needs")]
    pub needs: Needs,
    #[serde(rename = "if")]
    pub condition: Option<String>,
    #[serde(default)]
    pub outputs: HashMap<String, String>,
    #[serde(default)]
    pub steps: Vec<RawStep>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub concurrency: Option<serde_yaml::Value>,
    #[serde(default)]
    pub services: HashMap<String, serde_yaml::Value>,
    pub permissions: Option<serde_yaml::Value>,
    pub environment: Option<serde_yaml::Value>,
    pub defaults: Option<serde_yaml::Value>,
    /// Kept as an opaque value; support-tier detection (e.g. `strategy.matrix`)
    /// happens in the `ir` normalization layer.
    pub strategy: Option<serde_yaml::Value>,
    /// Job-level `uses:` (reusable workflow call). Presence flags the job as
    /// `Deferred` in the `ir` normalization layer.
    #[serde(default)]
    pub uses: Option<String>,
    /// Inputs passed to a reusable workflow call.
    #[serde(default, rename = "with")]
    pub with_inputs: HashMap<String, serde_yaml::Value>,
    /// Anything else (`secrets`, `concurrency`, ...) is preserved here
    /// rather than dropped, so later layers can surface it instead of
    /// silently ignoring it.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawStep {
    pub name: Option<String>,
    pub id: Option<String>,
    pub uses: Option<String>,
    pub run: Option<String>,
    pub shell: Option<String>,
    #[serde(rename = "working-directory")]
    pub working_directory: Option<String>,
    #[serde(rename = "if")]
    pub condition: Option<String>,
    /// Inputs passed to an action.
    #[serde(default, rename = "with")]
    pub with_inputs: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid workflow YAML{}: {source}", path_suffix(.path))]
    Yaml {
        path: Option<PathBuf>,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("failed to read workflow file {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn path_suffix(path: &Option<PathBuf>) -> String {
    match path {
        Some(p) => format!(" ({})", p.display()),
        None => String::new(),
    }
}

pub fn parse_workflow_str(s: &str) -> Result<RawWorkflow, ParseError> {
    serde_yaml::from_str(s).map_err(|source| ParseError::Yaml { path: None, source })
}

pub fn parse_workflow_file(path: &Path) -> Result<RawWorkflow, ParseError> {
    let contents = fs::read_to_string(path).map_err(|source| ParseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_yaml::from_str(&contents).map_err(|source| ParseError::Yaml {
        path: Some(path.to_path_buf()),
        source,
    })
}

fn deserialize_needs<'de, D>(deserializer: D) -> Result<Needs, D::Error>
where
    D: Deserializer<'de>,
{
    struct NeedsVisitor;

    impl<'de> Visitor<'de> for NeedsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a string or a sequence of strings")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![v.to_string()])
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![v])
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Vec::new())
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                out.push(item);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_any(NeedsVisitor)
}
