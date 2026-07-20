use anyhow::Result;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::timeout;

pub async fn watch_contracts(contracts_dir: &Path) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(32);
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.blocking_send(res);
        },
        Config::default().with_poll_interval(Duration::from_millis(500)),
    )?;

    watcher.watch(contracts_dir, RecursiveMode::Recursive)?;

    const DEBOUNCE_MS: u64 = 300;

    while let Some(event) = rx.recv().await {
        if let Ok(e) = event {
            if e.paths
                .iter()
                .any(|p| p.extension().map(|x| x == "clar").unwrap_or(false))
            {
                // Debounce bursts of save/write events before regenerating.
                while let Ok(Some(next_event)) =
                    timeout(Duration::from_millis(DEBOUNCE_MS), rx.recv()).await
                {
                    if let Ok(next) = next_event {
                        let is_clar_change = next
                            .paths
                            .iter()
                            .any(|p| p.extension().map(|x| x == "clar").unwrap_or(false));
                        if !is_clar_change {
                            continue;
                        }
                    }
                }

                if let Err(e) = stacksdapp_codegen::generate_all_quiet().await {
                    stacksdapp_shell::error(format!(
                        "[{}] ✗ Contract bindings failed: {e}",
                        timestamp_now()
                    ));
                } else {
                    stacksdapp_shell::status(format!(
                        "[{}] ✓ Contract bindings updated",
                        timestamp_now()
                    ));
                }
            }
        }
    }

    Ok(())
}

fn timestamp_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let hours = (secs / 3600) % 24;
    let mins = (secs / 60) % 60;
    let s = secs % 60;
    format!("{hours:02}:{mins:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::timestamp_now;
    use stacksdapp_shell::{init, status, ColorMode, Format, Shell};

    #[test]
    fn watcher_status_respects_quiet_mode() {
        init(Shell {
            verbosity: 0,
            quiet: true,
            format: Format::Human,
            color: ColorMode::Never,
        });
        status(format!("[{}] ✓ Contract bindings updated", timestamp_now()));
    }
}
