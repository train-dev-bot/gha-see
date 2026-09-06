//! Best-effort evaluation of job/step `if:` conditions against a mock run
//! context, so the visualizer can show a plausible run/skip state for each
//! job and step without ever executing anything.
//!
//! The v2 expression engine supports common GitHub, matrix, needs, env,
//! vars, and inputs contexts; string/JSON/file functions; boolean operators;
//! and equality/ordered comparisons. Values that are unavailable offline
//! (including secrets and non-local `hashFiles` patterns) remain Unknown.
//!
//! This module deliberately does not depend on `graph::JobGraph`: dependency
//! order between **instances** is walked directly via `JobInstance::needs`
//! (fanned out to every upstream instance of a matrix-expanded base job, via
//! `WorkflowFile::instances`'s `base_id`), memoizing each instance's
//! resolved [`RunState`] as it's computed (with a cycle guard, since
//! `needs:` cycles are already reported by `graph::build_job_graph` and
//! shouldn't cause an infinite loop here too).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, PathBuf};

use crate::expr::extract_expressions;
use crate::findings::{condition_skips, condition_unknown, Finding, FindingTarget};
use crate::ir::{Step, SupportTier, WorkflowFile};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct GithubCtx {
    pub sha: String,
    pub repository: String,
    pub actor: String,
}

#[derive(Debug, Clone, Default)]
pub struct NeedStatus {
    pub result: String,
    pub outputs: BTreeMap<String, String>,
}

/// Mock run context used to resolve `if:` conditions.
#[derive(Debug, Clone)]
pub struct EvalContext {
    pub event_name: String,
    pub ref_name: String,
    pub github: GithubCtx,
    pub env: BTreeMap<String, String>,
    pub vars: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, String>,
    /// Names known to exist; values are intentionally never stored.
    pub secrets: BTreeSet<String>,
    pub needs: BTreeMap<String, NeedStatus>,
    pub repo_root: Option<PathBuf>,
}

impl EvalContext {
    /// A plausible default context: a plain `push` to `refs/heads/main`
    /// with no environment variables set.
    pub fn default_mock() -> Self {
        EvalContext {
            event_name: "push".to_string(),
            ref_name: "refs/heads/main".to_string(),
            github: GithubCtx {
                sha: "0000000000000000000000000000000000000000".to_string(),
                repository: "octo/repository".to_string(),
                actor: "octocat".to_string(),
            },
            env: BTreeMap::new(),
            vars: BTreeMap::new(),
            inputs: BTreeMap::new(),
            secrets: BTreeSet::new(),
            needs: BTreeMap::new(),
            repo_root: None,
        }
    }

    /// Default mock context plus any defaults declared by
    /// `workflow_dispatch.inputs`. When several analyzed workflows declare
    /// the same key, the first workflow's default wins deterministically.
    pub fn default_mock_for(workflows: &[WorkflowFile]) -> Self {
        let mut ctx = Self::default_mock();
        for workflow in workflows {
            for (name, value) in workflow.triggers.dispatch_defaults() {
                ctx.inputs.entry(name).or_insert(value);
            }
        }
        ctx
    }
}

/// Whether a job or step will run against a given [`EvalContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    WillRun,
    Skipped,
    Unknown,
    Deferred,
}

/// The result of evaluating a single expression body: definitely true,
/// definitely false, or outside the v1 subset this evaluator understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalResult {
    True,
    False,
    Unknown,
}

/// Evaluate every job instance (and step) in `wf` against `ctx`, returning
/// the resolved [`RunState`] for each and any `GHA_COND_*` findings raised
/// along the way.
///
/// Results are keyed by instance id (`"build"`, or `"test (node=18)"` for a
/// matrix instance) and, for steps, by `"<instance id>#<step index>"`
/// (`"build#0"`).
///
/// Instances tagged [`SupportTier::Deferred`] (either because the job
/// itself has an unevaluated construct, or its `strategy.matrix` couldn't
/// be expanded) become `RunState::Deferred` here without an extra
/// finding — `ir`/`matrix` already recorded the reason, and the analysis
/// pipeline turns those into `GHA_DEFERRED`/`GHA_MATRIX_*` findings, so this
/// layer doesn't spam duplicates.
pub fn evaluate(
    wf: &WorkflowFile,
    ctx: &EvalContext,
) -> (BTreeMap<String, RunState>, Vec<Finding>) {
    let mut states = BTreeMap::new();
    let mut findings = Vec::new();
    let mut in_progress = BTreeSet::new();

    let by_base = instances_by_base(wf);

    for instance_id in wf.instances.keys() {
        eval_instance(
            instance_id,
            wf,
            &by_base,
            ctx,
            &mut states,
            &mut in_progress,
            &mut findings,
        );
    }

    for (instance_id, instance) in &wf.instances {
        let job_state = *states.get(instance_id).unwrap_or(&RunState::Unknown);
        let needs = resolved_needs(instance_id, wf, &by_base, ctx, &states);
        let instance_ctx = context_with_inputs(ctx, &instance.inputs);
        for (idx, step) in instance.steps.iter().enumerate() {
            let target = FindingTarget::Step {
                file: wf.path.clone(),
                job: instance_id.clone(),
                step_idx: idx,
            };
            let step_state = eval_step(
                job_state,
                step,
                target,
                &instance_ctx,
                &instance.matrix,
                &needs,
                &mut findings,
            );
            states.insert(format!("{instance_id}#{idx}"), step_state);
        }
    }

    (states, findings)
}

/// Group instance ids by their `base_id`, so a `needs: [build]` entry (which
/// always names the base job id) can be fanned out to every instance of
/// `build`.
fn instances_by_base(wf: &WorkflowFile) -> BTreeMap<&str, Vec<&str>> {
    let mut by_base: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (instance_id, instance) in &wf.instances {
        by_base
            .entry(instance.base_id.as_str())
            .or_default()
            .push(instance_id.as_str());
    }
    by_base
}

/// Resolve `instance_id`'s [`RunState`], recursing into its `needs:` first
/// (fanned out to every upstream instance of each needed base job,
/// memoizing into `states` as it goes) so upstream skip/unknown states can
/// propagate downstream.
fn eval_instance(
    instance_id: &str,
    wf: &WorkflowFile,
    by_base: &BTreeMap<&str, Vec<&str>>,
    ctx: &EvalContext,
    states: &mut BTreeMap<String, RunState>,
    in_progress: &mut BTreeSet<String>,
    findings: &mut Vec<Finding>,
) -> RunState {
    if let Some(&state) = states.get(instance_id) {
        return state;
    }

    // A `needs:` cycle is already reported by `graph::build_job_graph`;
    // treat it as unresolved here rather than recursing forever.
    if in_progress.contains(instance_id) {
        return RunState::Unknown;
    }

    let Some(instance) = wf.instances.get(instance_id) else {
        // Dangling `needs:` reference — already flagged as GHA_MISSING_NEED
        // by `graph::build_job_graph`; nothing to resolve here.
        return RunState::Unknown;
    };

    in_progress.insert(instance_id.to_string());

    let mut any_needed_skipped = false;
    let mut any_needed_unknown = false;
    for need in &instance.needs {
        match by_base.get(need.as_str()) {
            Some(upstream_ids) => {
                for &up_id in upstream_ids {
                    match eval_instance(up_id, wf, by_base, ctx, states, in_progress, findings) {
                        RunState::Skipped => any_needed_skipped = true,
                        RunState::Unknown | RunState::Deferred => any_needed_unknown = true,
                        RunState::WillRun => {}
                    }
                }
            }
            None => {
                // Dangling `needs:` reference — already flagged as
                // GHA_MISSING_NEED by `graph::build_job_graph`.
                any_needed_unknown = true;
            }
        }
    }

    in_progress.remove(instance_id);

    let target = FindingTarget::Job {
        file: wf.path.clone(),
        job: instance_id.to_string(),
    };

    let state = if instance.support == SupportTier::Deferred {
        RunState::Deferred
    } else if any_needed_skipped {
        // Simplified GHA semantics: default `needs` requires every upstream
        // instance to succeed, so any skipped upstream instance skips this
        // one too, regardless of its own `if:`.
        RunState::Skipped
    } else {
        let needs = resolved_needs(instance_id, wf, by_base, ctx, states);
        let instance_ctx = context_with_inputs(ctx, &instance.inputs);
        match &instance.condition {
            None => resolve_ran(any_needed_unknown),
            Some(raw) => match eval_condition(raw, &instance_ctx, &instance.matrix, &needs) {
                EvalResult::False => {
                    findings.push(condition_skips(target, raw.clone()));
                    RunState::Skipped
                }
                EvalResult::Unknown => {
                    findings.push(condition_unknown(target));
                    RunState::Unknown
                }
                EvalResult::True => resolve_ran(any_needed_unknown),
            },
        }
    };

    states.insert(instance_id.to_string(), state);
    state
}

fn context_with_inputs(ctx: &EvalContext, inputs: &BTreeMap<String, String>) -> EvalContext {
    let mut merged = ctx.clone();
    merged.inputs.extend(inputs.clone());
    merged
}

/// A job whose own condition passed (or was absent) still can't be
/// confidently marked `WillRun` if an upstream `needs:` is itself
/// unresolved or deferred.
fn resolve_ran(any_needed_unknown: bool) -> RunState {
    if any_needed_unknown {
        RunState::Unknown
    } else {
        RunState::WillRun
    }
}

fn resolved_needs(
    instance_id: &str,
    wf: &WorkflowFile,
    by_base: &BTreeMap<&str, Vec<&str>>,
    ctx: &EvalContext,
    states: &BTreeMap<String, RunState>,
) -> BTreeMap<String, NeedStatus> {
    let mut needs = ctx.needs.clone();
    let Some(instance) = wf.instances.get(instance_id) else {
        return needs;
    };

    for base_id in &instance.needs {
        let Some(upstream_ids) = by_base.get(base_id.as_str()) else {
            continue;
        };
        let upstream_states: Vec<RunState> = upstream_ids
            .iter()
            .filter_map(|id| states.get(*id).copied())
            .collect();
        let result = aggregate_need_result(&upstream_states);
        if let Some(result) = result {
            needs.entry(base_id.clone()).or_default().result = result.to_string();
        }
    }
    needs
}

fn aggregate_need_result(states: &[RunState]) -> Option<&'static str> {
    if states.is_empty()
        || states
            .iter()
            .any(|s| matches!(s, RunState::Unknown | RunState::Deferred))
    {
        None
    } else if states.contains(&RunState::Skipped) {
        Some("skipped")
    } else {
        Some("success")
    }
}

/// Resolve a single step's [`RunState`] given the job's already-resolved
/// state: a step never runs if its job didn't, otherwise its own `if:` (if
/// any) can only narrow the job's state down to `Skipped`/`Unknown`.
fn eval_step(
    job_state: RunState,
    step: &Step,
    target: FindingTarget,
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
    findings: &mut Vec<Finding>,
) -> RunState {
    if matches!(job_state, RunState::Skipped | RunState::Deferred) {
        return job_state;
    }
    if step.support == SupportTier::Deferred {
        return RunState::Deferred;
    }

    match &step.condition {
        None => job_state,
        Some(raw) => match eval_condition(raw, ctx, matrix, needs) {
            EvalResult::False => {
                findings.push(condition_skips(target, raw.clone()));
                RunState::Skipped
            }
            EvalResult::Unknown => {
                findings.push(condition_unknown(target));
                RunState::Unknown
            }
            EvalResult::True => job_state,
        },
    }
}

/// Evaluate a raw `if:` field (with or without a `${{ }}` wrapper) against
/// `ctx` and the current instance's `matrix` map.
fn eval_condition(
    raw: &str,
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
) -> EvalResult {
    match expression_body(raw) {
        Some(body) => eval_expression_str(&body, ctx, matrix, needs),
        None => EvalResult::Unknown,
    }
}

/// GHA allows `if:` to omit the `${{ }}` wrapper entirely; when present,
/// unwrap via `expr::extract_expressions` (reusing that module's tolerant
/// parsing of the delimiters) rather than re-implementing it here.
fn expression_body(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.contains("${{") {
        extract_expressions(raw).into_iter().next()
    } else {
        Some(raw.to_string())
    }
}

fn eval_expression_str(
    body: &str,
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
) -> EvalResult {
    let Some(tokens) = tokenize(body) else {
        return EvalResult::Unknown;
    };

    let mut parser = Parser { tokens, pos: 0 };
    match parser.parse_expr() {
        Some(expr) if parser.pos == parser.tokens.len() => eval_expr(&expr, ctx, matrix, needs),
        _ => EvalResult::Unknown,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    LParen,
    RParen,
    Comma,
    And,
    Or,
    Not,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Str(String),
    Ident(String),
}

#[derive(Debug, Clone)]
enum ValueExpr {
    String(String),
    Number(f64),
    Bool(bool),
    Path(String),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone)]
enum Expr {
    Value(ValueExpr),
    Compare(ValueExpr, CmpOp, ValueExpr),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

fn tokenize(s: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = s.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        match c {
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                tokens.push(Token::And);
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                tokens.push(Token::Or);
                i += 2;
            }
            '=' if chars.get(i + 1) == Some(&'=') => {
                tokens.push(Token::Eq);
                i += 2;
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                tokens.push(Token::Ne);
                i += 2;
            }
            '<' if chars.get(i + 1) == Some(&'=') => {
                tokens.push(Token::Le);
                i += 2;
            }
            '>' if chars.get(i + 1) == Some(&'=') => {
                tokens.push(Token::Ge);
                i += 2;
            }
            '<' => {
                tokens.push(Token::Lt);
                i += 1;
            }
            '>' => {
                tokens.push(Token::Gt);
                i += 1;
            }
            '!' => {
                tokens.push(Token::Not);
                i += 1;
            }
            '\'' | '"' => {
                let quote = c;
                let mut value = String::new();
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == quote {
                        closed = true;
                        i += 1;
                        break;
                    }
                    if chars[i] == '\\' && chars.get(i + 1).is_some() {
                        i += 1;
                    }
                    value.push(chars[i]);
                    i += 1;
                }
                if !closed {
                    return None;
                }
                tokens.push(Token::Str(value));
            }
            c if is_ident_char(c) => {
                let start = i;
                let mut j = i;
                while j < chars.len() && is_ident_char(chars[j]) {
                    j += 1;
                }
                tokens.push(Token::Ident(chars[start..j].iter().collect()));
                i = j;
            }
            _ => return None,
        }
    }

    Some(tokens)
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, want: &Token) -> bool {
        if self.peek() == Some(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut lhs = self.parse_and()?;
        while self.eat(&Token::Or) {
            let rhs = self.parse_and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    fn parse_and(&mut self) -> Option<Expr> {
        let mut lhs = self.parse_unary()?;
        while self.eat(&Token::And) {
            let rhs = self.parse_unary()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        if self.eat(&Token::Not) {
            let inner = self.parse_unary()?;
            return Some(Expr::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        if self.eat(&Token::LParen) {
            let inner = self.parse_expr()?;
            return if self.eat(&Token::RParen) {
                Some(inner)
            } else {
                None
            };
        }

        let lhs = self.parse_value()?;
        let op = if self.eat(&Token::Eq) {
            Some(CmpOp::Eq)
        } else if self.eat(&Token::Ne) {
            Some(CmpOp::Ne)
        } else if self.eat(&Token::Le) {
            Some(CmpOp::Le)
        } else if self.eat(&Token::Lt) {
            Some(CmpOp::Lt)
        } else if self.eat(&Token::Ge) {
            Some(CmpOp::Ge)
        } else if self.eat(&Token::Gt) {
            Some(CmpOp::Gt)
        } else {
            None
        };
        match op {
            Some(op) => Some(Expr::Compare(lhs, op, self.parse_value()?)),
            None => Some(Expr::Value(lhs)),
        }
    }

    fn parse_value(&mut self) -> Option<ValueExpr> {
        match self.bump()? {
            Token::Str(s) => Some(ValueExpr::String(s)),
            Token::Ident(name) => {
                if self.eat(&Token::LParen) {
                    let mut args = Vec::new();
                    if !self.eat(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.eat(&Token::RParen) {
                                break;
                            }
                            if !self.eat(&Token::Comma) {
                                return None;
                            }
                        }
                    }
                    Some(ValueExpr::Call(name, args))
                } else if name.eq_ignore_ascii_case("true") {
                    Some(ValueExpr::Bool(true))
                } else if name.eq_ignore_ascii_case("false") {
                    Some(ValueExpr::Bool(false))
                } else if let Ok(number) = name.parse::<f64>() {
                    Some(ValueExpr::Number(number))
                } else {
                    Some(ValueExpr::Path(name))
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    Json(serde_json::Value),
    Unknown,
}

fn eval_value(
    value: &ValueExpr,
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
) -> Value {
    match value {
        ValueExpr::String(value) => Value::String(value.clone()),
        ValueExpr::Number(value) => Value::Number(*value),
        ValueExpr::Bool(value) => Value::Bool(*value),
        ValueExpr::Path(path) => resolve_path(path, ctx, matrix, needs),
        ValueExpr::Call(name, args) => eval_call(name, args, ctx, matrix, needs),
    }
}

fn resolve_path(
    path: &str,
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
) -> Value {
    let value = match path {
        "github.event_name" => Some(ctx.event_name.clone()),
        "github.ref" => Some(ctx.ref_name.clone()),
        "github.ref_name" => Some(
            ctx.ref_name
                .strip_prefix("refs/heads/")
                .or_else(|| ctx.ref_name.strip_prefix("refs/tags/"))
                .unwrap_or(&ctx.ref_name)
                .to_string(),
        ),
        "github.sha" => Some(ctx.github.sha.clone()),
        "github.repository" => Some(ctx.github.repository.clone()),
        "github.actor" => Some(ctx.github.actor.clone()),
        _ if path.starts_with("secrets.") => return Value::Unknown,
        _ => path
            .strip_prefix("env.")
            .and_then(|key| ctx.env.get(key).cloned())
            .or_else(|| {
                path.strip_prefix("vars.")
                    .and_then(|key| ctx.vars.get(key).cloned())
            })
            .or_else(|| {
                path.strip_prefix("inputs.")
                    .and_then(|key| ctx.inputs.get(key).cloned())
            })
            .or_else(|| {
                path.strip_prefix("matrix.")
                    .and_then(|key| matrix.get(key).cloned())
            })
            .or_else(|| resolve_need_path(path, needs)),
    };
    value.map(Value::String).unwrap_or(Value::Unknown)
}

fn resolve_need_path(path: &str, needs: &BTreeMap<String, NeedStatus>) -> Option<String> {
    let rest = path.strip_prefix("needs.")?;
    let (job, field) = rest.split_once('.')?;
    let need = needs.get(job)?;
    if field == "result" {
        (!need.result.is_empty()).then(|| need.result.clone())
    } else {
        field
            .strip_prefix("outputs.")
            .and_then(|name| need.outputs.get(name).cloned())
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
) -> Value {
    let values: Vec<Value> = args
        .iter()
        .map(|arg| eval_expr_value(arg, ctx, matrix, needs))
        .collect();
    match name.to_ascii_lowercase().as_str() {
        "success" | "always" if values.is_empty() => Value::Bool(true),
        "failure" | "cancelled" if values.is_empty() => Value::Bool(false),
        "contains" if values.len() == 2 => contains_value(&values[0], &values[1]),
        "startswith" if values.len() == 2 => string_predicate(&values, |a, b| a.starts_with(b)),
        "endswith" if values.len() == 2 => string_predicate(&values, |a, b| a.ends_with(b)),
        "format" if !values.is_empty() => format_value(&values),
        "join" if values.len() == 2 => join_value(&values[0], &values[1]),
        "tojson" if values.len() == 1 => to_json_value(&values[0]),
        "fromjson" if values.len() == 1 => from_json_value(&values[0]),
        "hashfiles" if !values.is_empty() => hash_files(&values, ctx),
        _ => Value::Unknown,
    }
}

fn eval_expr_value(
    expr: &Expr,
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
) -> Value {
    match expr {
        Expr::Value(value) => eval_value(value, ctx, matrix, needs),
        _ => match eval_expr(expr, ctx, matrix, needs) {
            EvalResult::True => Value::Bool(true),
            EvalResult::False => Value::Bool(false),
            EvalResult::Unknown => Value::Unknown,
        },
    }
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Json(value) => match value {
            serde_json::Value::String(value) => Some(value.clone()),
            _ => serde_json::to_string(value).ok(),
        },
        Value::Unknown => None,
    }
}

fn contains_value(haystack: &Value, needle: &Value) -> Value {
    let Some(needle) = value_string(needle) else {
        return Value::Unknown;
    };
    let needle = needle.to_ascii_lowercase();
    match haystack {
        Value::Json(serde_json::Value::Array(items)) => Value::Bool(items.iter().any(|item| {
            value_string(&Value::Json(item.clone()))
                .is_some_and(|item| item.eq_ignore_ascii_case(&needle))
        })),
        other => match value_string(other) {
            Some(haystack) => Value::Bool(haystack.to_ascii_lowercase().contains(&needle)),
            None => Value::Unknown,
        },
    }
}

fn string_predicate(values: &[Value], predicate: impl FnOnce(&str, &str) -> bool) -> Value {
    match (value_string(&values[0]), value_string(&values[1])) {
        (Some(a), Some(b)) => {
            Value::Bool(predicate(&a.to_ascii_lowercase(), &b.to_ascii_lowercase()))
        }
        _ => Value::Unknown,
    }
}

fn format_value(values: &[Value]) -> Value {
    let Some(mut output) = value_string(&values[0]) else {
        return Value::Unknown;
    };
    for (index, value) in values.iter().skip(1).enumerate() {
        let Some(value) = value_string(value) else {
            return Value::Unknown;
        };
        output = output.replace(&format!("{{{index}}}"), &value);
    }
    Value::String(output)
}

fn join_value(array: &Value, separator: &Value) -> Value {
    let Some(separator) = value_string(separator) else {
        return Value::Unknown;
    };
    let parsed;
    let items = match array {
        Value::Json(serde_json::Value::Array(items)) => items,
        Value::String(value) => {
            parsed = match serde_json::from_str::<serde_json::Value>(value) {
                Ok(serde_json::Value::Array(items)) => items,
                _ => return Value::Unknown,
            };
            &parsed
        }
        _ => return Value::Unknown,
    };
    if items.is_empty() {
        return Value::String(String::new());
    }
    let values: Option<Vec<String>> = items
        .iter()
        .map(|item| value_string(&Value::Json(item.clone())))
        .collect();
    let Some(values) = values else {
        return Value::Unknown;
    };
    Value::String(values.join(&separator))
}

fn to_json_value(value: &Value) -> Value {
    let json = match value {
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Number(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Json(value) => value.clone(),
        Value::Unknown => return Value::Unknown,
    };
    serde_json::to_string(&json)
        .map(Value::String)
        .unwrap_or(Value::Unknown)
}

fn from_json_value(value: &Value) -> Value {
    let Some(value) = value_string(value) else {
        return Value::Unknown;
    };
    serde_json::from_str(&value)
        .map(Value::Json)
        .unwrap_or(Value::Unknown)
}

fn hash_files(patterns: &[Value], ctx: &EvalContext) -> Value {
    let Some(root) = ctx.repo_root.as_ref() else {
        return Value::Unknown;
    };
    let Ok(root) = root.canonicalize() else {
        return Value::Unknown;
    };
    let mut files = BTreeSet::new();
    for pattern in patterns {
        let Some(pattern) = value_string(pattern) else {
            return Value::Unknown;
        };
        let relative = PathBuf::from(&pattern);
        if relative.is_absolute()
            || relative.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Value::Unknown;
        }
        let Some(full_pattern) = root.join(relative).to_str().map(str::to_owned) else {
            return Value::Unknown;
        };
        let Ok(matches) = glob::glob(&full_pattern) else {
            return Value::Unknown;
        };
        for path in matches.flatten() {
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            if path.starts_with(&root) && path.is_file() {
                files.insert(path);
            }
        }
    }
    if files.is_empty() {
        return Value::Unknown;
    }
    let mut combined = Sha256::new();
    for file in files {
        let Ok(contents) = fs::read(file) else {
            return Value::Unknown;
        };
        combined.update(Sha256::digest(contents));
    }
    Value::String(format!("{:x}", combined.finalize()))
}

fn eval_expr(
    expr: &Expr,
    ctx: &EvalContext,
    matrix: &BTreeMap<String, String>,
    needs: &BTreeMap<String, NeedStatus>,
) -> EvalResult {
    match expr {
        Expr::Value(value) => value_truth(&eval_value(value, ctx, matrix, needs)),
        Expr::Compare(lhs, op, rhs) => compare_values(
            &eval_value(lhs, ctx, matrix, needs),
            *op,
            &eval_value(rhs, ctx, matrix, needs),
        ),
        Expr::Not(inner) => match eval_expr(inner, ctx, matrix, needs) {
            EvalResult::True => EvalResult::False,
            EvalResult::False => EvalResult::True,
            EvalResult::Unknown => EvalResult::Unknown,
        },
        Expr::And(lhs, rhs) => {
            let l = eval_expr(lhs, ctx, matrix, needs);
            if l == EvalResult::False {
                return EvalResult::False;
            }
            match (l, eval_expr(rhs, ctx, matrix, needs)) {
                (_, EvalResult::False) => EvalResult::False,
                (EvalResult::True, EvalResult::True) => EvalResult::True,
                _ => EvalResult::Unknown,
            }
        }
        Expr::Or(lhs, rhs) => {
            let l = eval_expr(lhs, ctx, matrix, needs);
            if l == EvalResult::True {
                return EvalResult::True;
            }
            match (l, eval_expr(rhs, ctx, matrix, needs)) {
                (_, EvalResult::True) => EvalResult::True,
                (EvalResult::False, EvalResult::False) => EvalResult::False,
                _ => EvalResult::Unknown,
            }
        }
    }
}

fn value_truth(value: &Value) -> EvalResult {
    match value {
        Value::Bool(true) => EvalResult::True,
        Value::Bool(false) => EvalResult::False,
        Value::String(value) => {
            if value.is_empty() {
                EvalResult::False
            } else {
                EvalResult::True
            }
        }
        Value::Number(value) => {
            if *value == 0.0 {
                EvalResult::False
            } else {
                EvalResult::True
            }
        }
        Value::Json(serde_json::Value::Null) => EvalResult::False,
        Value::Json(_) => EvalResult::True,
        Value::Unknown => EvalResult::Unknown,
    }
}

fn compare_values(lhs: &Value, op: CmpOp, rhs: &Value) -> EvalResult {
    if matches!(lhs, Value::Unknown) || matches!(rhs, Value::Unknown) {
        return EvalResult::Unknown;
    }
    let matched = match op {
        CmpOp::Eq | CmpOp::Ne => {
            let equal = match (lhs, rhs) {
                (Value::Number(a), Value::Number(b)) => a == b,
                (Value::Bool(a), Value::Bool(b)) => a == b,
                (Value::Json(a), Value::Json(b)) => a == b,
                _ => match (value_string(lhs), value_string(rhs)) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
                    _ => return EvalResult::Unknown,
                },
            };
            if matches!(op, CmpOp::Eq) {
                equal
            } else {
                !equal
            }
        }
        CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => {
            match (coerce_number(lhs), coerce_number(rhs)) {
                (Some(a), Some(b)) => ordered(op, a.partial_cmp(&b)),
                _ => match (value_string(lhs), value_string(rhs)) {
                    (Some(a), Some(b)) => ordered(op, Some(a.cmp(&b))),
                    _ => return EvalResult::Unknown,
                },
            }
        }
    };
    if matched {
        EvalResult::True
    } else {
        EvalResult::False
    }
}

fn coerce_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn ordered(op: CmpOp, ordering: Option<Ordering>) -> bool {
    let Some(ordering) = ordering else {
        return false;
    };
    match op {
        CmpOp::Lt => ordering == Ordering::Less,
        CmpOp::Le => ordering != Ordering::Greater,
        CmpOp::Gt => ordering == Ordering::Greater,
        CmpOp::Ge => ordering != Ordering::Less,
        CmpOp::Eq | CmpOp::Ne => unreachable!(),
    }
}
