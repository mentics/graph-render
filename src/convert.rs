//! Project supergraphs down to the node/edge views that nov-viz renders.
//!
//! A full supergraph holds every statement, expression and value, which is far
//! too much to draw. Each view here is a purpose-built diagram:
//!
//! * **Modules** — source files, with an edge wherever code in one file calls
//!   code in another (or in an external package).
//! * **Call graph** — functions and methods, with an edge for every call whose
//!   target is known (a project callable or an external one).

use std::collections::{HashMap, HashSet};

use nov_viz::{NovEdge, NovGraph, NovNode, NovView};
use petgraph::stable_graph::NodeIndex;
use supergraph::supergraph::{EdgeFact, NodeFact, ProgramSupergraph};

/// Edge types; nov-viz gives each a distinct colour and legend entry.
const CALLS: &str = "calls";
const CALLS_EXTERNAL: &str = "calls_external";

pub struct ViewSummary {
    pub name: &'static str,
    pub text: String,
}

pub fn to_views(graphs: &[ProgramSupergraph], max_nodes: usize) -> (Vec<NovView>, Vec<ViewSummary>) {
    let mut views = Vec::new();
    let mut summaries = Vec::new();
    for (name, build) in [
        ("Modules", module_diagram as fn(&[ProgramSupergraph]) -> Diagram),
        ("Call graph", call_graph),
    ] {
        let (graph, text) = build(graphs).into_nov(max_nodes);
        views.push(NovView::new(name, graph));
        summaries.push(ViewSummary { name, text });
    }
    (views, summaries)
}

struct Candidate {
    id: String,
    label: String,
    tag: &'static str,
    /// Lower sorts first and survives the `max_nodes` cap longer.
    rank: u8,
}

#[derive(Default)]
struct Diagram {
    candidates: Vec<Candidate>,
    seen_nodes: HashSet<String>,
    links: Vec<(String, String, &'static str)>,
    seen_links: HashSet<(String, String)>,
}

impl Diagram {
    fn node(&mut self, id: &str, label: &str, tag: &'static str, rank: u8) {
        if self.seen_nodes.insert(id.to_string()) {
            self.candidates.push(Candidate { id: id.to_string(), label: label.to_string(), tag, rank });
        }
    }

    fn link(&mut self, source: &str, target: &str, kind: &'static str) {
        if source != target && self.seen_links.insert((source.to_string(), target.to_string())) {
            self.links.push((source.to_string(), target.to_string(), kind));
        }
    }

    /// Drop nodes that nothing links to or from, except those `keep` accepts.
    fn retain_connected(&mut self, keep: impl Fn(&Candidate) -> bool) {
        let connected: HashSet<&str> =
            self.links.iter().flat_map(|(s, t, _)| [s.as_str(), t.as_str()]).collect();
        self.candidates.retain(|c| keep(c) || connected.contains(c.id.as_str()));
    }

    fn into_nov(mut self, max_nodes: usize) -> (NovGraph, String) {
        // Links whose endpoints aren't real nodes (e.g. dangling ids) are dropped.
        let seen = &self.seen_nodes;
        self.links.retain(|(s, t, _)| seen.contains(s) && seen.contains(t));

        let total_nodes = self.candidates.len();
        let total_links = self.links.len();

        let mut degree: HashMap<&str, usize> = HashMap::new();
        for (s, t, _) in &self.links {
            *degree.entry(s).or_default() += 1;
            *degree.entry(t).or_default() += 1;
        }
        // Project nodes first, then by connectivity, then id for determinism.
        self.candidates.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| degree.get(b.id.as_str()).cmp(&degree.get(a.id.as_str())))
                .then_with(|| a.id.cmp(&b.id))
        });
        self.candidates.truncate(max_nodes);

        let mut nov = NovGraph::new();
        let mut index: HashMap<&str, NodeIndex> = HashMap::new();
        for c in &self.candidates {
            let node = NovNode::new(c.id.clone(), c.label.clone()).with_tags([c.tag]).with_size(nov_viz::CARD_SIZE.0 as f64, nov_viz::CARD_SIZE.1 as f64);
            index.insert(&c.id, nov.add_node(node));
        }
        let mut edge_count = 0;
        for (n, (s, t, kind)) in self.links.iter().enumerate() {
            if let (Some(&si), Some(&ti)) = (index.get(s.as_str()), index.get(t.as_str())) {
                nov.add_edge(si, ti, NovEdge::new(format!("e{n}"), *kind));
                edge_count += 1;
            }
        }

        let mut summary = format!(
            "{} of {total_nodes} nodes, {edge_count} of {total_links} edges",
            self.candidates.len()
        );
        if total_nodes > max_nodes {
            summary.push_str(&format!(" (capped at --max-nodes {max_nodes})"));
        }
        (nov, summary)
    }
}

/// The package an external target belongs to, falling back to its leading path segment.
fn external_package(package: &Option<String>, qualified_name: &str) -> String {
    package
        .clone()
        .unwrap_or_else(|| qualified_name.split(['.', ':']).next().unwrap_or(qualified_name).to_string())
}

fn module_diagram(graphs: &[ProgramSupergraph]) -> Diagram {
    let mut d = Diagram::default();
    for graph in graphs {
        let mut artifact_of: HashMap<&str, &str> = HashMap::new();
        let mut externals: HashMap<&str, String> = HashMap::new();
        for node in &graph.nodes {
            match &node.fact {
                NodeFact::Artifact(a) => d.node(&a.artifact_id, &a.path, "module", 0),
                NodeFact::Callable(c) => {
                    artifact_of.insert(&c.callable_id, &c.artifact_id);
                }
                NodeFact::ExternalTarget(t) => {
                    externals.insert(&t.external_target_id, external_package(&t.package_name, &t.qualified_name));
                }
                _ => {}
            }
        }
        for edge in &graph.edges {
            let EdgeFact::Calls(call) = &edge.fact else { continue };
            let Some(&from) = artifact_of.get(call.caller_callable_id.as_str()) else { continue };
            if let Some(callee) = &call.callee_callable_id {
                if let Some(&to) = artifact_of.get(callee.as_str()) {
                    d.link(from, to, CALLS);
                }
            } else if let Some(target) = &call.external_target_id
                && let Some(package) = externals.get(target.as_str())
            {
                let id = format!("package:{package}");
                d.node(&id, package, "package", 1);
                d.link(from, &id, CALLS_EXTERNAL);
            }
        }
    }
    d
}

fn call_graph(graphs: &[ProgramSupergraph]) -> Diagram {
    let mut d = Diagram::default();
    for graph in graphs {
        for node in &graph.nodes {
            match &node.fact {
                NodeFact::Callable(c) => d.node(&c.callable_id, &c.qualified_name, "function", 0),
                NodeFact::ExternalTarget(t) => d.node(&t.external_target_id, &t.qualified_name, "external", 1),
                _ => {}
            }
        }
        for edge in &graph.edges {
            let EdgeFact::Calls(call) = &edge.fact else { continue };
            if let Some(target) = &call.callee_callable_id {
                d.link(&call.caller_callable_id, target, CALLS);
            } else if let Some(target) = &call.external_target_id {
                d.link(&call.caller_callable_id, target, CALLS_EXTERNAL);
            }
        }
    }
    // A callable that neither calls nor is called says nothing about the call structure.
    d.retain_connected(|_| false);
    d
}
