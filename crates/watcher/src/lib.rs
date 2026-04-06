use anyhow::Result;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;

pub async fn watch_contracts(contracts_dir: &Path) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(32);
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.blocking_send(res);
        },
        Config::default().with_poll_interval(Duration::from_millis(500)),
    )?;

    watcher.watch(contracts_dir, RecursiveMode::Recursive)?;

    while let Some(event) = rx.recv().await {
        if let Ok(e) = event {
            if e.paths.iter().any(|p| {
                p.extension()
                    .map(|x| x == "clar")
                    .unwrap_or(false)
            }) {
                println!("[watcher] .clar change detected — regenerating...");
                if let Err(e) = stacksdapp_codegen::generate_all().await {
                    eprintln!("[watcher] codegen error: {e}");
                }
            }
        }
    }

    Ok(())
}
