//! Background scan of converted prop models for the editor browser.
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, block_on, futures_lite::future::poll_once},
};
use std::path::Path;

#[derive(Resource, Default)]
pub(super) struct ModelBrowser {
    pub open: bool,
    pub query: String,
    pub models: Vec<String>,
    scan: Option<Task<Vec<String>>>,
}

pub(super) fn start_model_scan(browser: &mut ModelBrowser) {
    if browser.scan.is_some() || !browser.models.is_empty() {
        return;
    }
    browser.scan = Some(
        IoTaskPool::get().spawn(async { scan_converted_models(Path::new("assets/data/models")) }),
    );
}

pub(super) fn poll_model_scan(mut browser: ResMut<ModelBrowser>) {
    let Some(task) = browser.scan.as_mut() else {
        return;
    };
    let Some(models) = block_on(poll_once(task)) else {
        return;
    };
    browser.scan = None;
    browser.models = models;
}

pub(super) fn model_scan_active(browser: &ModelBrowser) -> bool {
    browser.scan.is_some()
}

fn scan_converted_models(root: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    scan_directory(root, root, &mut paths);
    paths.sort_unstable();
    paths
}

fn scan_directory(root: &Path, directory: &Path, paths: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_directory(root, &path, paths);
        } else if path.extension().is_some_and(|extension| extension == "glb")
            && let Ok(relative) = path.strip_prefix(root)
        {
            paths.push(format!(
                "ro://models/{}",
                relative.to_string_lossy().replace('\\', "/")
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_returns_sorted_ro_model_paths() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("nested")).unwrap();
        std::fs::write(directory.path().join("z.glb"), []).unwrap();
        std::fs::write(directory.path().join("nested/a.glb"), []).unwrap();
        std::fs::write(directory.path().join("ignored.txt"), []).unwrap();

        assert_eq!(
            scan_converted_models(directory.path()),
            vec!["ro://models/nested/a.glb", "ro://models/z.glb"]
        );
    }
}
