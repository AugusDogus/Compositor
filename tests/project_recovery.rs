use compositor::{
    document::{Document, LayerContent},
    project,
};
use image::{Rgba, RgbaImage};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

struct SaveProcess(Child);

impl Drop for SaveProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn writing_asset(parent: &Path) -> Option<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .find_map(|entry| {
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".compositor-save-")
            {
                return None;
            }
            let stage = entry.path();
            if stage.join("manifest.json").exists() {
                return None;
            }
            let writing = fs::read_dir(stage.join("images"))
                .ok()?
                .filter_map(|entry| entry.ok())
                .any(|asset| asset.metadata().is_ok_and(|m| m.len() >= 65_536));
            writing.then_some(stage)
        })
}

#[test]
fn killed_save_preserves_the_last_committed_project() {
    const CHILD_PATH: &str = "COMPOSITOR_RECOVERY_TEST_CHILD";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let path = Path::new(&path);
        let mut document = project::load(path).unwrap();
        // Incompressible pixels keep the real PNG write in progress long enough for
        // the parent to observe it. No production failpoint or artificial save pause.
        let mut noise = 0x12345678_u32;
        let pixels = RgbaImage::from_fn(3000, 3000, |_, _| {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            Rgba(noise.to_le_bytes())
        });
        document.layers[0].content = LayerContent::Raster(Some(Arc::new(pixels)));
        project::save(&document, path).unwrap();
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("recover.comp");
    let mut document = Document::new(2, 2).unwrap();
    compositor::edits::fill(&mut document, [10, 20, 30, 255], false, false).unwrap();
    project::save(&document, &path).unwrap();
    let manifest = fs::read(path.join("manifest.json")).unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "killed_save_preserves_the_last_committed_project",
            "--nocapture",
        ])
        .env(CHILD_PATH, &path)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut child = SaveProcess(child);
    let deadline = Instant::now() + Duration::from_secs(15);
    let abandoned = loop {
        if let Some(stage) = writing_asset(directory.path()) {
            break stage;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Save exited before its in-progress asset could be observed"
        );
        assert!(
            Instant::now() < deadline,
            "Save did not begin writing an asset within 15 seconds"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    assert!(!abandoned.join("manifest.json").exists());
    assert_eq!(fs::read(path.join("manifest.json")).unwrap(), manifest);
    assert_eq!(project::load(&path).unwrap(), document);
    compositor::edits::fill(&mut document, [40, 50, 60, 255], false, false).unwrap();
    project::save(&document, &path).unwrap();
    assert_eq!(project::load(&path).unwrap(), document);
}
