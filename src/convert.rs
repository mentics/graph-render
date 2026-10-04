//! Project supergraphs down to the node/edge view that nov-viz renders.
//!
//! A full supergraph holds every statement, expression and value, which is far
//! too much to draw. The rendered view keeps only source files (artifacts) and
//! callables, linked by "contains" and "calls" edges.

use std::collections::{HashMap, HashSet};

use nov_viz::{NovEdge, NovGraph, NovNode};
use petgraph::stable_graph::NodeIndex;
use supergraph::supergraph::{EdgeFact, NodeFact, ProgramSupergraph};

struct Candidate {
    id: String,
    label: String,
    tag: &'static str,
    artifact: bool,
}

pub fn to_nov_graph(graphs: &[ProgramSupergraph], max_nodes: usize) -> (NovGraph, String) {
    let mut candidates: Vec<Candidate> = Vec::new();
    // (source, target, edge type) in candidate id space, deduplicated.
    let mut links: Vec<(String, String, &'static str)> = Vec::new();
    let mut seen_links: HashSet<(String, String, &'static str)> = HashSet::new();
    let mut seen_nodes: HashSet<String> = HashSet::new();

    for graph in graphs {
        for node in &graph.nodes {
            match &node.fact {
                NodeFact::Artifact(a) => {
                    if seen_nodes.insert(a.artifact_id.clone()) {
                        candidates.push(Candidate {
                            id: a.artifact_id.clone(),
                            label: a.path.clone(),
                            tag: "file",
                            artifact: true,
                        });
                    }
                }
                NodeFact::Callable(c) => {
                    if seen_nodes.insert(c.callable_id.clone()) {
                        candidates.push(Candidate {
                            id: c.callable_id.clone(),
                            label: c.qualified_name.clone(),
                            tag: "callable",
                            artifact: false,
                        });
                        let link = (c.artifact_id.clone(), c.callable_id.clone(), "contains");
                        if seen_links.insert(link.clone()) {
                            links.push(link);
                        }
                    }
                }
                _ => {}
            }
        }
        for edge in &graph.edges {
            if let EdgeFact::Calls(call) = &edge.fact
                && let Some(callee) = &call.callee_callable_id
            {
                let link = (call.caller_callable_id.clone(), callee.clone(), "calls");
                if seen_links.insert(link.clone()) {
                    links.push(link);
                }
            }
        }
    }

    // Links whose endpoints aren't real candidates (e.g. dangling ids) are dropped.
    links.retain(|(s, t, _)| seen_nodes.contains(s) && seen_nodes.contains(t));

    let total_nodes = candidates.len();
    let total_links = links.len();

    let mut degree: HashMap<&str, usize> = HashMap::new();
    for (s, t, _) in &links {
        *degree.entry(s).or_default() += 1;
        *degree.entry(t).or_default() += 1;
    }
    // Files first (they anchor the structure), then by connectivity, then id for determinism.
    candidates.sort_by(|a, b| {
        b.artifact
            .cmp(&a.artifact)
            .then_with(|| degree.get(b.id.as_str()).cmp(&degree.get(a.id.as_str())))
            .then_with(|| a.id.cmp(&b.id))
    });
    candidates.truncate(max_nodes);

    let mut nov = NovGraph::new();
    let mut index: HashMap<String, NodeIndex> = HashMap::new();
    for c in &candidates {
        let idx = nov.add_node(NovNode::new(c.id.clone(), c.label.clone()).with_tags([c.tag]).with_size(200.0, 56.0));
        index.insert(c.id.clone(), idx);
    }
    let mut edge_count = 0;
    for (n, (s, t, kind)) in links.iter().enumerate() {
        if let (Some(&si), Some(&ti)) = (index.get(s), index.get(t)) {
            nov.add_edge(si, ti, NovEdge::new(format!("e{n}"), *kind));
            edge_count += 1;
        }
    }

    let mut summary = format!(
        "rendering {} of {total_nodes} nodes, {edge_count} of {total_links} edges",
        candidates.len()
    );
    if total_nodes > max_nodes {
        summary.push_str(&format!(" (capped at --max-nodes {max_nodes})"));
    }
    (nov, summary)
}
