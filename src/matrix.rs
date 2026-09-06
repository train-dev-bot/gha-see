//! Expand `strategy.matrix` (cartesian product, `include`, `exclude`) into
//! concrete [`crate::ir::JobInstance`]s.
//!
//! v1 scope: matrix axis values (and `include`/`exclude` entry values) must
//! be plain YAML scalars, stringified via [`scalar_to_string`]. A matrix
//! that's an expression (e.g. `${{ fromJson(...) }}`), or whose axis values
//! aren't scalars, can't be expanded here — [`expand_matrix`] returns
//! [`MatrixError::Unsupported`] and the caller leaves the job `Deferred`
//! with a single fallback instance rather than guessing.
//!
//! Order of operations (matches the design doc, not verbatim upstream GHA
//! semantics, since this is a best-effort v1): cartesian product of the
//! non-`include`/`exclude` axes, then `include` (merge into matching
//! combos, or append as a new combo if nothing matches), then `exclude`
//! (drop any combo that's a superset of an exclude entry), then cap at
//! [`MAX_INSTANCES`].

use std::collections::{BTreeMap, BTreeSet};

use serde_yaml::Value;

use crate::ir::{Job, JobInstance, SupportTier};

/// Hard cap on instances expanded per job (matches the design doc; roughly
/// mirrors GitHub Actions' own per-workflow-run job limits closely enough
/// for a v1 visualizer).
pub const MAX_INSTANCES: usize = 256;

/// One concrete matrix combination: axis name -> stringified scalar value.
pub type Combo = BTreeMap<String, String>;

/// Why a `strategy.matrix` couldn't be expanded in v1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixError {
    Unsupported(String),
}

impl MatrixError {
    pub fn reason(&self) -> &str {
        match self {
            MatrixError::Unsupported(reason) => reason,
        }
    }
}

/// Recorded on `Job::matrix_note` when a job's matrix needed a
/// `GHA_MATRIX_*` finding; `analysis::collect_matrix_findings` turns this
/// into the actual [`crate::findings::Finding`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixNote {
    /// The matrix product was empty (e.g. every combination was removed by
    /// `exclude`, or an axis had zero values). The job is left `Deferred`
    /// with a single fallback instance since nothing would ever run.
    Empty,
    /// The matrix expanded to more combinations than [`MAX_INSTANCES`];
    /// only the first `cap` are kept as instances (still `Supported`).
    Capped { total: usize, cap: usize },
    /// The matrix shape isn't expandable in v1 (see [`MatrixError`]). The
    /// job is left `Deferred` with a single fallback instance.
    Unsupported(String),
}

/// The result of successfully parsing (not yet capping) a `strategy.matrix`.
#[derive(Debug)]
struct Expansion {
    /// Final combinations, already capped to [`MAX_INSTANCES`].
    combos: Vec<Combo>,
    /// How many combinations existed before capping (equal to
    /// `combos.len()` when not capped).
    total_before_cap: usize,
}

/// Expand `strategy`'s `matrix:` key, if any.
///
/// - `Ok(None)`: no `strategy`, or a `strategy` with no `matrix:` key (e.g.
///   only `fail-fast`/`max-parallel`) — caller should treat the job as a
///   single, non-matrix instance.
/// - `Ok(Some(expansion))`: matrix understood and expanded (`combos` may be
///   empty if every combination was excluded).
/// - `Err`: matrix shape isn't expandable in v1 — caller should leave the
///   job `Deferred`.
fn expand_matrix(strategy: Option<&Value>) -> Result<Option<Expansion>, MatrixError> {
    let Some(strategy) = strategy else {
        return Ok(None);
    };
    let Some(mapping) = strategy.as_mapping() else {
        return Err(MatrixError::Unsupported(
            "strategy is not a mapping".to_string(),
        ));
    };
    let Some(matrix_value) = mapping.get(Value::String("matrix".to_string())) else {
        return Ok(None);
    };

    let matrix_mapping = matrix_value.as_mapping().ok_or_else(|| {
        MatrixError::Unsupported(
            "strategy.matrix is not a mapping (dynamic/expression matrices aren't evaluated in v1)"
                .to_string(),
        )
    })?;

    let mut axes: Vec<(String, Vec<String>)> = Vec::new();
    let mut include: Vec<Combo> = Vec::new();
    let mut exclude: Vec<Combo> = Vec::new();

    for (key, value) in matrix_mapping {
        let key_str = key.as_str().ok_or_else(|| {
            MatrixError::Unsupported("strategy.matrix key is not a string".to_string())
        })?;
        match key_str {
            "include" => include = parse_combo_list(value)?,
            "exclude" => exclude = parse_combo_list(value)?,
            _ => {
                let values = parse_scalar_seq(value).ok_or_else(|| {
                    MatrixError::Unsupported(format!(
                        "strategy.matrix.{key_str} is not a list of plain scalar values"
                    ))
                })?;
                axes.push((key_str.to_string(), values));
            }
        }
    }

    let axis_names: BTreeSet<String> = axes.iter().map(|(k, _)| k.clone()).collect();
    let mut combos = cartesian_product(&axes);
    apply_include(&mut combos, &include, &axis_names);
    apply_exclude(&mut combos, &exclude);

    let total_before_cap = combos.len();
    combos.truncate(MAX_INSTANCES);

    Ok(Some(Expansion {
        combos,
        total_before_cap,
    }))
}

/// A plain YAML scalar (not a sequence/mapping/tagged value), stringified.
fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("null".to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Sequence(_) | Value::Mapping(_) | Value::Tagged(_) => None,
    }
}

fn parse_scalar_seq(value: &Value) -> Option<Vec<String>> {
    let seq = value.as_sequence()?;
    seq.iter().map(scalar_to_string).collect()
}

fn parse_combo_list(value: &Value) -> Result<Vec<Combo>, MatrixError> {
    let seq = value.as_sequence().ok_or_else(|| {
        MatrixError::Unsupported("strategy.matrix include/exclude is not a list".to_string())
    })?;
    seq.iter()
        .map(|entry| {
            let mapping = entry.as_mapping().ok_or_else(|| {
                MatrixError::Unsupported(
                    "strategy.matrix include/exclude entry is not a mapping".to_string(),
                )
            })?;
            let mut combo = Combo::new();
            for (k, v) in mapping {
                let key = k.as_str().ok_or_else(|| {
                    MatrixError::Unsupported(
                        "strategy.matrix include/exclude key is not a string".to_string(),
                    )
                })?;
                let val = scalar_to_string(v).ok_or_else(|| {
                    MatrixError::Unsupported(format!(
                        "strategy.matrix include/exclude value for `{key}` is not a plain scalar"
                    ))
                })?;
                combo.insert(key.to_string(), val);
            }
            Ok(combo)
        })
        .collect()
}

fn cartesian_product(axes: &[(String, Vec<String>)]) -> Vec<Combo> {
    if axes.is_empty() {
        return Vec::new();
    }
    let mut combos: Vec<Combo> = vec![Combo::new()];
    for (key, values) in axes {
        let mut next = Vec::with_capacity(combos.len() * values.len());
        for combo in &combos {
            for value in values {
                let mut c = combo.clone();
                c.insert(key.clone(), value.clone());
                next.push(c);
            }
        }
        combos = next;
    }
    combos
}

/// Merge (or append) `include` entries into `combos`, best-effort per the
/// design doc:
/// - If `axis_names` is empty (the matrix has no base axes — it's defined
///   purely by `include`), every entry becomes its own combo.
/// - Otherwise, an entry whose keys don't overlap any axis name is merged
///   into *every* existing combo (e.g. adding a `color` field to a
///   `fruit`-only matrix). An entry that does overlap an axis name is
///   merged into every combo matching on those overlapping keys, or
///   appended as a brand new combo if nothing matches.
fn apply_include(combos: &mut Vec<Combo>, include: &[Combo], axis_names: &BTreeSet<String>) {
    if axis_names.is_empty() {
        for entry in include {
            combos.push(entry.clone());
        }
        return;
    }

    for entry in include {
        let matching_keys: Vec<&String> =
            entry.keys().filter(|k| axis_names.contains(*k)).collect();

        if matching_keys.is_empty() {
            for combo in combos.iter_mut() {
                for (k, v) in entry {
                    combo.insert(k.clone(), v.clone());
                }
            }
            continue;
        }

        let mut matched = false;
        for combo in combos.iter_mut() {
            if matching_keys
                .iter()
                .all(|k| combo.get(k.as_str()) == entry.get(k.as_str()))
            {
                matched = true;
                for (k, v) in entry {
                    combo.insert(k.clone(), v.clone());
                }
            }
        }
        if !matched {
            combos.push(entry.clone());
        }
    }
}

/// Drop every combo that any `exclude` entry is a subset-match of.
fn apply_exclude(combos: &mut Vec<Combo>, exclude: &[Combo]) {
    combos.retain(|combo| !exclude.iter().any(|entry| is_subset_match(entry, combo)));
}

/// Whether every key/value pair in `entry` also appears in `combo`.
fn is_subset_match(entry: &Combo, combo: &Combo) -> bool {
    entry.iter().all(|(k, v)| combo.get(k) == Some(v))
}

/// Expand every job in `jobs` into its [`JobInstance`]s, setting each job's
/// `matrix_note` along the way (`None` when there's nothing to report).
///
/// Every job gets at least one instance, even when its matrix is
/// unsupported or empty, so `needs:` fan-out (`graph`/`eval`) always has
/// something to point at.
pub fn expand_workflow(jobs: &mut BTreeMap<String, Job>) -> BTreeMap<String, JobInstance> {
    let mut instances = BTreeMap::new();

    for (base_id, job) in jobs.iter_mut() {
        match expand_matrix(job.strategy.as_ref()) {
            Ok(None) => {
                job.matrix_note = None;
                let inst = single_instance(base_id, job);
                instances.insert(inst.instance_id.clone(), inst);
            }
            Ok(Some(expansion)) if expansion.combos.is_empty() => {
                job.matrix_note = Some(MatrixNote::Empty);
                job.support = SupportTier::Deferred;
                let mut inst = single_instance(base_id, job);
                inst.support = SupportTier::Deferred;
                inst.deferred_reasons.push(
                    "strategy.matrix produced zero combinations (after `exclude`, or an empty axis); \
                     no instance of this job will run"
                        .to_string(),
                );
                instances.insert(inst.instance_id.clone(), inst);
            }
            Ok(Some(expansion)) => {
                let capped = expansion.total_before_cap > expansion.combos.len();
                job.matrix_note = if capped {
                    Some(MatrixNote::Capped {
                        total: expansion.total_before_cap,
                        cap: MAX_INSTANCES,
                    })
                } else {
                    None
                };
                for combo in expansion.combos {
                    let inst = instance_for_combo(base_id, job, combo);
                    instances.insert(inst.instance_id.clone(), inst);
                }
            }
            Err(err) => {
                let reason = err.reason().to_string();
                job.matrix_note = Some(MatrixNote::Unsupported(reason.clone()));
                job.support = SupportTier::Deferred;
                let mut inst = single_instance(base_id, job);
                inst.support = SupportTier::Deferred;
                inst.deferred_reasons
                    .push(format!("strategy.matrix not evaluated in v1: {reason}"));
                instances.insert(inst.instance_id.clone(), inst);
            }
        }
    }

    instances
}

fn single_instance(base_id: &str, job: &Job) -> JobInstance {
    instance_for_combo(base_id, job, Combo::new())
}

fn instance_for_combo(base_id: &str, job: &Job, combo: Combo) -> JobInstance {
    let instance_id = format_instance_id(base_id, &combo);
    JobInstance {
        instance_id,
        base_id: base_id.to_string(),
        matrix: combo,
        inputs: job.with_inputs.clone(),
        runs_on: job.runs_on.clone(),
        needs: job.needs.clone(),
        condition: job.condition.clone(),
        outputs: job.outputs.clone(),
        env: job.env.clone(),
        strategy: job.strategy.clone(),
        concurrency: job.concurrency.clone(),
        services: job.services.clone(),
        permissions: job.permissions.clone(),
        environment: job.environment.clone(),
        defaults_run: job.defaults_run.clone(),
        steps: job.steps.clone(),
        support: job.support,
        deferred_reasons: job.deferred_reasons.clone(),
    }
}

fn format_instance_id(base_id: &str, combo: &Combo) -> String {
    if combo.is_empty() {
        return base_id.to_string();
    }
    let pairs: Vec<String> = combo.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{base_id} ({})", pairs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix_yaml(body: &str) -> Value {
        serde_yaml::from_str(body).unwrap()
    }

    #[test]
    fn no_strategy_yields_none() {
        assert!(expand_matrix(None).unwrap().is_none());
    }

    #[test]
    fn strategy_without_matrix_yields_none() {
        let strategy = matrix_yaml("fail-fast: false\n");
        assert!(expand_matrix(Some(&strategy)).unwrap().is_none());
    }

    #[test]
    fn two_axis_cartesian_product() {
        let strategy =
            matrix_yaml("matrix:\n  node: [18, 20]\n  os: [ubuntu-latest, windows-latest]\n");
        let expansion = expand_matrix(Some(&strategy)).unwrap().unwrap();
        assert_eq!(expansion.combos.len(), 4);
        assert_eq!(expansion.total_before_cap, 4);
        assert!(expansion.combos.iter().any(|c| {
            c.get("node").map(String::as_str) == Some("18")
                && c.get("os").map(String::as_str) == Some("ubuntu-latest")
        }));
    }

    #[test]
    fn include_merges_into_matching_and_appends_new() {
        let strategy = matrix_yaml(
            "matrix:\n  fruit: [apple, pear]\n  include:\n    - color: green\n    - fruit: banana\n      color: yellow\n",
        );
        let expansion = expand_matrix(Some(&strategy)).unwrap().unwrap();
        assert_eq!(expansion.combos.len(), 3);
        assert!(expansion
            .combos
            .iter()
            .any(|c| c.get("fruit").map(String::as_str) == Some("apple")
                && c.get("color").map(String::as_str) == Some("green")));
        assert!(expansion
            .combos
            .iter()
            .any(|c| c.get("fruit").map(String::as_str) == Some("pear")
                && c.get("color").map(String::as_str) == Some("green")));
        assert!(expansion
            .combos
            .iter()
            .any(|c| c.get("fruit").map(String::as_str) == Some("banana")
                && c.get("color").map(String::as_str) == Some("yellow")));
    }

    #[test]
    fn include_only_matrix_creates_one_combo_per_entry() {
        let strategy = matrix_yaml("matrix:\n  include:\n    - node: 18\n    - node: 20\n");
        let expansion = expand_matrix(Some(&strategy)).unwrap().unwrap();
        assert_eq!(expansion.combos.len(), 2);
    }

    #[test]
    fn exclude_drops_matching_combo() {
        let strategy = matrix_yaml(
            "matrix:\n  os: [ubuntu-latest, windows-latest]\n  node: [18, 20]\n  exclude:\n    - os: windows-latest\n      node: 18\n",
        );
        let expansion = expand_matrix(Some(&strategy)).unwrap().unwrap();
        assert_eq!(expansion.combos.len(), 3);
        assert!(!expansion.combos.iter().any(|c| {
            c.get("os").map(String::as_str) == Some("windows-latest")
                && c.get("node").map(String::as_str) == Some("18")
        }));
    }

    #[test]
    fn exclude_of_every_combo_yields_empty() {
        let strategy = matrix_yaml("matrix:\n  node: [18]\n  exclude:\n    - node: 18\n");
        let expansion = expand_matrix(Some(&strategy)).unwrap().unwrap();
        assert!(expansion.combos.is_empty());
    }

    #[test]
    fn oversize_matrix_is_capped() {
        let strategy = matrix_yaml("matrix:\n  a: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n  b: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]\n");
        let expansion = expand_matrix(Some(&strategy)).unwrap().unwrap();
        assert_eq!(expansion.total_before_cap, 320);
        assert_eq!(expansion.combos.len(), MAX_INSTANCES);
    }

    #[test]
    fn expression_valued_matrix_is_unsupported() {
        let strategy = matrix_yaml("matrix: \"${{ fromJson(needs.setup.outputs.matrix) }}\"\n");
        let err = expand_matrix(Some(&strategy)).unwrap_err();
        assert!(matches!(err, MatrixError::Unsupported(_)));
    }

    #[test]
    fn non_scalar_axis_value_is_unsupported() {
        let strategy = matrix_yaml("matrix:\n  config:\n    - name: a\n      value: 1\n");
        let err = expand_matrix(Some(&strategy)).unwrap_err();
        assert!(matches!(err, MatrixError::Unsupported(_)));
    }
}
