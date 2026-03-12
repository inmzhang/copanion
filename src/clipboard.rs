use std::io::Write as _;

use anyhow::{Result, anyhow};
use arboard::Clipboard;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

pub fn copy_text(text: &str) -> Result<&'static str> {
    if should_prefer_osc52() {
        copy_osc52(text)?;
        return Ok("copied via terminal clipboard");
    }

    match Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(_) => Ok("copied to clipboard"),
        Err(_) => {
            copy_osc52(text)?;
            Ok("copied via terminal clipboard")
        }
    }
}

fn should_prefer_osc52() -> bool {
    std::env::var("TMUX").is_ok()
        || std::env::var("SSH_TTY").is_ok()
        || std::env::var("ZELLIJ").is_ok()
}

fn copy_osc52(text: &str) -> Result<()> {
    if std::env::var("TMUX").is_ok() {
        copy_via_tmux(text)
    } else {
        let mut stdout = std::io::stdout().lock();
        write_osc52(&mut stdout, text)
    }
}

fn copy_via_tmux(text: &str) -> Result<()> {
    use std::process::{Command, Stdio};

    let mut child = Command::new("tmux")
        .args(["load-buffer", "-w", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| anyhow!("failed to launch tmux for clipboard export: {err}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|err| anyhow!("failed to feed tmux clipboard buffer: {err}"))?;
    }

    let status = child
        .wait()
        .map_err(|err| anyhow!("failed to wait on tmux clipboard command: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("tmux clipboard command exited with {status}"))
    }
}

fn write_osc52<W: std::io::Write>(writer: &mut W, text: &str) -> Result<()> {
    let encoded = BASE64.encode(text);
    write!(writer, "\x1b]52;c;{encoded}\x07")
        .map_err(|err| anyhow!("failed to write OSC 52 sequence: {err}"))?;
    writer
        .flush()
        .map_err(|err| anyhow!("failed to flush OSC 52 sequence: {err}"))
}
