mod convert;

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use nov_viz::{Projection, layout};
use supergraph::supergraph::ProgramSupergraph;

/// Parse the code in a repository or workspace into a supergraph and render it.
#[derive(Parser)]
#[command(name = "graph-render", version)]
struct Cli {
    /// Repository, workspace directory, or single source file to analyze.
    path: PathBuf,

    /// Maximum number of nodes to render; the most-connected nodes are kept.
    #[arg(long, default_value_t = 1000)]
    max_nodes: usize,

    /// Build the graph and run layout, print a summary, but don't open a window.
    #[arg(long)]
    no_window: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !cli.path.exists() {
        bail!("{} does not exist", cli.path.display());
    }

    let mut graphs: Vec<ProgramSupergraph> = Vec::new();
    for language in ["python", "typescript", "rust"] {
        let graph = match language {
            "python" => supergraph::analyze_python_supergraph(&cli.path)?,
            "typescript" => supergraph::analyze_typescript_supergraph(&cli.path)?,
            _ => supergraph::analyze_rust_supergraph(&cli.path)?,
        };
        eprintln!(
            "{language}: {} supergraph nodes, {} edges",
            graph.nodes.len(),
            graph.edges.len()
        );
        if !graph.nodes.is_empty() {
            graphs.push(graph);
        }
    }
    if graphs.is_empty() {
        bail!(
            "no supported source files found under {} (supported: Python .py, TypeScript .ts/.tsx, Rust .rs)",
            cli.path.display()
        );
    }

    let (views, summaries) = convert::to_views(&graphs, cli.max_nodes);
    for summary in &summaries {
        eprintln!("{}: {}", summary.name, summary.text);
    }

    let started = std::time::Instant::now();
    if cli.no_window {
        for (view, summary) in views.iter().zip(&summaries) {
            let laid_out = layout(&view.graph, Projection::Flow)?;
            eprintln!(
                "{} layout ok: {} nodes, {} edges in {:.2?}",
                summary.name,
                laid_out.nodes.len(),
                laid_out.edges.len(),
                started.elapsed()
            );
        }
        return Ok(());
    }
    nov_viz::show_views(views, &format!("graph-render — {}", cli.path.display()))
}
