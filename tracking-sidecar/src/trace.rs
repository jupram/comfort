use crate::types::IpcMessage;
use anyhow::Context;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct TraceWriter {
    writer: BufWriter<File>,
}

impl TraceWriter {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        let file = File::create(path).with_context(|| format!("create trace file {:?}", path))?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    pub fn write(&mut self, msg: &IpcMessage) -> anyhow::Result<()> {
        serde_json::to_writer(&mut self.writer, msg).context("serialize trace message")?;
        self.writer
            .write_all(b"\n")
            .context("write trace newline")?;
        Ok(())
    }

    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.writer.flush().context("flush trace")?;
        Ok(())
    }
}
