use crate::types::IpcMessage;
use anyhow::Context;
use std::io::{self, Write};

pub fn emit(msg: &IpcMessage) -> anyhow::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, msg).context("serialize ipc message")?;
    out.write_all(b"\n").context("write newline")?;
    out.flush().context("flush stdout")?;
    Ok(())
}
