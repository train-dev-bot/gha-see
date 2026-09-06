//! Build a job dependency graph from a normalized [`WorkflowFile`], surfacing
//! `needs:` cycles and dangling references as [`Finding`]s rather than
//! failing outright, so the rest of the workflow can still be rendered.
//!
//! Nodes are **instances**, not logical (YAML) jobs: a `needs: [build]`
//! entry — whose `build` always names the base job id, matrix or not — fans
//! out into an edge from every instance of `build` to every instance of the
//! job that needs it. A job without `strategy.matrix` has exactly one
//! instance (`instance_id == base_id`), so this fan-out degenerates to the
//! familiar one-to-one edges for the common case.

use std::collections::{BTreeMap, BTreeSet};

use petgraph::graph::NodeIndex;
use petgraph::Direction;
use petgraph::Graph;

use crate::findings::{cycle, missing_need, Finding};
use crate::ir::WorkflowFile;

/// A job dependency graph: nodes are instance ids, edges run from a needed
/// instance to the instance that needs it (`needs:` means an edge from the
/// upstream/needed instance to the downstream/dependent instance).
#[derive(Clone)]
pub struct JobGraph {
    pub graph: Graph<String, ()>,
    /// Instance id -> node index, so callers can look up an instance's
    /// place in `graph` without a linear scan.
    pub nodes: BTreeMap<String, NodeIndex>,
}

/// Build the job graph for `wf`.
///
/// Emits `GHA_MISSING_NEED` (once per distinct `(base job, missing name)`
/// pair, even if that base job expanded to many instances) for every
/// `needs:` entry that doesn't name a job defined in this file, and
/// `GHA_CYCLE` for every `needs:` dependency cycle found among instances.
/// The graph is always returned — even when cyclic or missing edges — so
/// downstream consumers (e.g. rendering) can still show whatever structure
/// exists.
pub fn build_job_graph(wf: &WorkflowFile) -> (JobGraph, Vec<Finding>) {
    let mut graph = Graph::<String, ()>::new();
    let mut nodes = BTreeMap::new();
    let mut findings = Vec::new();

    for instance_id in wf.instances.keys() {
        let idx = graph.add_node(instance_id.clone());
        nodes.insert(instance_id.clone(), idx);
    }

    let mut by_base: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (instance_id, instance) in &wf.instances {
        by_base
            .entry(instance.base_id.as_str())
            .or_default()
            .push(instance_id.as_str());
    }

    let mut reported_missing: BTreeSet<(String, String)> = BTreeSet::new();

    for (instance_id, instance) in &wf.instances {
        for need in &instance.needs {
            match by_base.get(need.as_str()) {
                Some(upstream_ids) => {
                    let down_idx = nodes[instance_id];
                    for &up_id in upstream_ids {
                        graph.add_edge(nodes[up_id], down_idx, ());
                    }
                }
                None => {
                    let key = (instance.base_id.clone(), need.clone());
                    if reported_missing.insert(key) {
                        findings.push(missing_need(
                            wf.path.clone(),
                            instance.base_id.clone(),
                            need.clone(),
                        ));
                    }
                }
            }
        }
    }

    for scc in petgraph::algo::tarjan_scc(&graph) {
        let is_cycle = scc.len() > 1 || scc.iter().any(|&n| graph.contains_edge(n, n));
        if is_cycle {
            let mut jobs: Vec<String> = scc.iter().map(|&n| graph[n].clone()).collect();
            jobs.sort();
            findings.push(cycle(wf.path.clone(), &jobs));
        }
    }

    (JobGraph { graph, nodes }, findings)
}

/// Group job ids into indentation levels for tree rendering.
///
/// Jobs with no `needs:` are roots (level 0); each subsequent level holds
/// jobs whose `needs:` are all satisfied by jobs in earlier levels. Job
/// names within a level are sorted for deterministic output.
///
/// Jobs that are part of a `needs:` cycle never reach in-degree zero and so
/// have no well-defined depth; they're simply omitted rather than causing an
/// infinite loop or a panic.
pub fn topo_levels(graph: &JobGraph) -> Vec<Vec<String>> {
    let g = &graph.graph;

    let mut remaining: BTreeMap<NodeIndex, usize> = g
        .node_indices()
        .map(|idx| (idx, g.neighbors_directed(idx, Direction::Incoming).count()))
        .collect();

    let mut current: Vec<NodeIndex> = remaining
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&idx, _)| idx)
        .collect();

    let mut levels = Vec::new();

    while !current.is_empty() {
        let mut names: Vec<String> = current.iter().map(|&idx| g[idx].clone()).collect();
        names.sort();
        levels.push(names);

        let mut next = Vec::new();
        for &idx in &current {
            for succ in g.neighbors_directed(idx, Direction::Outgoing) {
                if let Some(deg) = remaining.get_mut(&succ) {
                    *deg -= 1;
                    if *deg == 0 {
                        next.push(succ);
                    }
                }
            }
        }
        current = next;
    }

    levels
}
