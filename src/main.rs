use std::net::SocketAddr;

use anyhow::Result;
use clap::Parser;

use skimdiff::gitx::Repo;
use skimdiff::server::{router, AppState};

/// Lightweight diff review for agent-made changes. Run with no arguments in a
/// repo for a live working-tree view, or pass a range (`main..feat`, `HEAD~3`,
/// a sha) for a fixed diff.
#[derive(Parser)]
#[command(name = "skimdiff", version)]
struct Cli {
    /// Commit range or single commit to diff (default: live working tree)
    range: Option<String>,

    /// Port to listen on (0 = pick a free port)
    #[arg(long, default_value_t = 4400)]
    port: u16,

    /// Don't open the browser automatically
    #[arg(long)]
    no_open: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo = Repo::discover(&std::env::current_dir()?)?;
    println!("repo: {}", repo.root.display());

    let app = router(AppState::new(repo, cli.range.clone()));

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", cli.port)).await {
        Ok(l) => l,
        // port taken: let the OS pick one
        Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?,
    };
    let addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://{addr}");
    match &cli.range {
        Some(r) => println!("skimdiff — {r} — {url}"),
        None => println!("skimdiff — live working tree — {url}"),
    }
    if !cli.no_open {
        let _ = open::that(&url);
    }
    axum::serve(listener, app).await?;
    Ok(())
}
