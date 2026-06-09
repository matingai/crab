use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn stop_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("stop")
}

fn stop_path(data_dir: &Path, session_id: &str) -> PathBuf {
    stop_root(data_dir).join(format!("{session_id}.stop"))
}

pub fn request_stop(data_dir: &Path, session_id: &str) -> Result<()> {
    let root = stop_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create runtime stop dir {}", root.display()))?;
    let path = stop_path(data_dir, session_id);
    fs::write(&path, b"stop")
        .with_context(|| format!("failed to write stop request {}", path.display()))?;
    Ok(())
}

pub fn clear_stop_request(data_dir: &Path, session_id: &str) -> Result<()> {
    let path = stop_path(data_dir, session_id);
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(())
}

pub fn stop_requested(data_dir: &Path, session_id: &str) -> bool {
    stop_path(data_dir, session_id).is_file()
}

#[cfg(test)]
mod tests {
    use super::{clear_stop_request, request_stop, stop_requested};

    #[test]
    fn creates_and_clears_stop_requests() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(!stop_requested(tmp.path(), "demo"));
        request_stop(tmp.path(), "demo").expect("request stop");
        assert!(stop_requested(tmp.path(), "demo"));
        clear_stop_request(tmp.path(), "demo").expect("clear stop");
        assert!(!stop_requested(tmp.path(), "demo"));
    }
}
