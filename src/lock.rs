use crate::{generation::render_template, manifest::Dependency};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolvedDependency {
    pub id: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DependencyInput {
    pub config: Dependency,
    pub trees: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GenerationInputs {
    pub generator: String,
    pub durable_roots: Vec<String>,
    pub dependencies: Vec<DependencyInput>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GenerationState {
    pub inputs: GenerationInputs,
    pub output: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SyncRecord {
    pub version: u32,
    pub dependencies: Vec<ResolvedDependency>,
    pub state: GenerationState,
}

pub(crate) fn sync_lock(root: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".skillctl.lock"))?;
    if let Err(error) = FileExt::try_lock_exclusive(&file) {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            bail!("another sync is in progress");
        }

        return Err(error).context("cannot lock synchronization");
    }

    Ok(file)
}

pub(crate) fn render_sync_lock(record: &SyncRecord) -> Result<Vec<u8>> {
    let content = render_template("lock.json.j2", serde_json::to_value(record)?)?;
    serde_json::from_slice::<SyncRecord>(&content)
        .context("lock template produced invalid JSON")?;
    Ok(content)
}

pub(crate) fn write_sync_lock(file: &mut File, content: &[u8]) -> Result<()> {
    // Keep the locked inode stable. A torn write is rejected on the next read.
    file.seek(SeekFrom::Start(0))?;
    file.write_all(content)?;
    file.set_len(content.len() as u64)?;
    file.sync_all()?;
    Ok(())
}
