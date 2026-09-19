//! Runs `$VISUAL`/`$EDITOR` on a temp file (the caller suspends the TUI).
use std::process::Command;

use anyhow::{bail, Context, Result};

pub fn edit_text(initial: &str) -> Result<String> {
    let path = std::env::temp_dir().join(format!("roadie-{}.md", std::process::id()));
    std::fs::write(&path, initial).context("cannot write temp file")?;
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let mut parts = editor.split_whitespace();
    let prog = parts.next().unwrap_or("vi");
    let status = Command::new(prog).args(parts).arg(&path).status();
    let result = match status {
        Ok(s) if s.success() => std::fs::read_to_string(&path).context("cannot read edited file"),
        Ok(s) => Err(anyhow::anyhow!("editor exited with {s}")),
        Err(e) => Err(anyhow::anyhow!("cannot run {prog}: {e}")),
    };
    let _ = std::fs::remove_file(&path);
    if let Ok(t) = &result {
        if t.trim().is_empty() && !initial.trim().is_empty() {
            bail!("editor returned empty text; keeping the original");
        }
    }
    result
}
