//! Watches the open folder so the app follows the AI (or any editor) writing specs (doc 08: the
//! file is the contract). Bursts of file system events are coalesced before anything is emitted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::Duration;

use notify::event::{EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use dioramic_packs::PROJECT_ICON_DIR;

use crate::error::CommandError;
use crate::workspace::is_spec_file;

/// Quiet period that closes a burst (editors and the MCP write in several syscalls).
const DEBOUNCE: Duration = Duration::from_millis(120);

/// What a burst of events means for the app.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WatchBatch {
    /// Spec files whose content may have changed.
    pub changed_specs: BTreeSet<PathBuf>,
    /// Files or folders appeared, disappeared or moved: rescan the tree.
    pub tree_changed: bool,
    /// Project `icons/` folders whose content changed: render again with their icons.
    pub changed_icon_dirs: BTreeSet<PathBuf>,
}

impl WatchBatch {
    /// True when nothing relevant happened.
    pub fn is_empty(&self) -> bool {
        self.changed_specs.is_empty() && !self.tree_changed && self.changed_icon_dirs.is_empty()
    }

    /// Folds one event into the batch.
    pub fn absorb(&mut self, event: &Event) {
        let touches_specs = event.paths.iter().any(|path| is_spec_file(path));
        let structural =
            matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)));
        if structural && (touches_specs || event.paths.iter().any(|path| is_folder_like(path))) {
            self.tree_changed = true;
        }
        if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
            self.changed_specs.extend(event.paths.iter().filter(|path| is_spec_file(path)).cloned());
        }
        if !matches!(event.kind, EventKind::Access(_)) {
            self.changed_icon_dirs.extend(event.paths.iter().filter_map(|path| project_icon_dir(path)));
        }
    }
}

/// The project `icons/` folder a path is, or is directly inside (`/w/icons/logo.svg` → `/w/icons`).
fn project_icon_dir(path: &Path) -> Option<PathBuf> {
    let is_icon_dir = |dir: &Path| dir.file_name().is_some_and(|name| name == PROJECT_ICON_DIR);
    if is_icon_dir(path) {
        return Some(path.to_path_buf());
    }
    path.parent().filter(|parent| is_icon_dir(parent)).map(Path::to_path_buf)
}

/// Paths without an extension are probably folders (a removed folder can no longer be stat'ed).
fn is_folder_like(path: &Path) -> bool {
    path.is_dir() || path.extension().is_none()
}

/// Starts watching `root` recursively; `on_batch` runs on a background thread for each burst.
/// Dropping the returned watcher stops both the watcher and the thread.
pub fn watch(root: &Path, on_batch: impl Fn(WatchBatch) + Send + 'static) -> Result<RecommendedWatcher, CommandError> {
    let (sender, receiver) = channel::<Event>();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        if let Ok(event) = result {
            // The receiver is gone only while the app shuts down.
            let _ = sender.send(event);
        }
    })
    .map_err(|error| CommandError::Watch(error.to_string()))?;
    watcher.watch(root, RecursiveMode::Recursive).map_err(|error| CommandError::Watch(error.to_string()))?;
    thread::Builder::new()
        .name("dioramic-watch".into())
        .spawn(move || coalesce(&receiver, &on_batch))
        .map_err(|error| CommandError::Watch(error.to_string()))?;
    Ok(watcher)
}

fn coalesce(receiver: &Receiver<Event>, on_batch: &impl Fn(WatchBatch)) {
    while let Ok(first) = receiver.recv() {
        let mut batch = WatchBatch::default();
        batch.absorb(&first);
        loop {
            match receiver.recv_timeout(DEBOUNCE) {
                Ok(event) => batch.absorb(&event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        if !batch.is_empty() {
            on_batch(batch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, RemoveKind};
    use std::sync::mpsc;

    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn content_change_of_a_spec_is_not_structural() {
        let mut batch = WatchBatch::default();
        batch.absorb(&event(EventKind::Modify(ModifyKind::Data(DataChange::Content)), "/w/a.diorama.yaml"));
        assert!(!batch.tree_changed);
        assert_eq!(batch.changed_specs.len(), 1);
    }

    #[test]
    fn new_and_removed_specs_rescan_the_tree() {
        let mut batch = WatchBatch::default();
        batch.absorb(&event(EventKind::Create(CreateKind::File), "/w/new.diorama.yaml"));
        assert!(batch.tree_changed);
        let mut batch = WatchBatch::default();
        batch.absorb(&event(EventKind::Remove(RemoveKind::Any), "/w/gone-folder"));
        assert!(batch.tree_changed);
    }

    #[test]
    fn unrelated_files_are_ignored() {
        let mut batch = WatchBatch::default();
        batch.absorb(&event(EventKind::Modify(ModifyKind::Data(DataChange::Content)), "/w/readme.md"));
        batch.absorb(&event(EventKind::Create(CreateKind::File), "/w/notes.txt"));
        assert!(batch.is_empty());
    }

    #[test]
    fn project_icon_changes_are_reported_by_folder() {
        let mut batch = WatchBatch::default();
        batch.absorb(&event(EventKind::Create(CreateKind::File), "/w/icons/logo.svg"));
        batch.absorb(&event(EventKind::Modify(ModifyKind::Data(DataChange::Content)), "/w/icons/manifest.json"));
        batch.absorb(&event(EventKind::Remove(RemoveKind::Any), "/w/sub/icons"));
        batch.absorb(&event(EventKind::Modify(ModifyKind::Data(DataChange::Content)), "/w/icons/deep/x.svg"));
        let dirs: Vec<_> = batch.changed_icon_dirs.iter().map(|dir| dir.display().to_string()).collect();
        assert_eq!(dirs, ["/w/icons", "/w/sub/icons"]);
        assert!(batch.changed_specs.is_empty());
    }

    #[test]
    fn watcher_reports_a_written_spec() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        let (sender, receiver) = mpsc::channel();
        let _watcher = watch(&root, move |batch| {
            let _ = sender.send(batch);
        })?;
        // Give FSEvents a moment to arm before writing.
        thread::sleep(Duration::from_millis(300));
        let spec = root.join("live.diorama.yaml");
        std::fs::write(&spec, "diorama: 1\n")?;
        let batch = receiver.recv_timeout(Duration::from_secs(5))?;
        assert!(batch.changed_specs.iter().any(|path| path.ends_with("live.diorama.yaml")));
        Ok(())
    }
}
