//! `dioramic watch`: render, then re-render after every save of the spec (debounced).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use anyhow::{Context, Result};
use dioramic_render::Engine;
use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::cli::RenderArgs;
use crate::commands::render::{RenderFailure, RenderJob};
use crate::ui::{self, CYAN, DIM, UserError, paint};

/// Quiet period that closes a burst of file events (editors save in several steps).
const DEBOUNCE: Duration = Duration::from_millis(150);

type EventResult = notify::Result<Event>;

/// Runs until the process is interrupted.
pub fn run(args: &RenderArgs) -> Result<()> {
    let job = RenderJob::from_args(args)?;
    if !job.spec.is_file() {
        return Err(UserError::new(format!("spec not found: {}", job.spec.display()))
            .hint("watch needs an existing file; create it first")
            .into());
    }
    let engine = Engine::shared();
    render_once(&job, engine);

    let (sender, receiver) = mpsc::channel::<EventResult>();
    let mut watcher = notify::recommended_watcher(sender).context("cannot start the file watcher")?;
    // Watch the directory, not the file: editors often replace the file (write temp + rename),
    // which would silently end a watch on the old inode.
    let directory = watched_directory(&job.spec);
    watcher
        .watch(&directory, RecursiveMode::NonRecursive)
        .with_context(|| format!("cannot watch {}", directory.display()))?;
    anstream::eprintln!("{} {} {}", paint(CYAN, "watching"), job.spec.display(), paint(DIM, "(Ctrl+C to stop)"));

    let spec_name = job.spec.file_name().map(OsString::from).unwrap_or_default();
    while wait_for_change(&receiver, &spec_name, DEBOUNCE) {
        anstream::eprintln!("{}", paint(DIM, "↻ change detected"));
        render_once(&job, engine);
    }
    Ok(())
}

/// Renders; every failure (spec errors, a file missing mid-save, a write error) is printed and the
/// watcher keeps waiting for the next save.
fn render_once(job: &RenderJob, engine: &Engine) {
    match job.render(engine) {
        Ok(()) | Err(RenderFailure::Reported) => {}
        Err(RenderFailure::Other(error)) => ui::report_error(&error),
    }
}

fn watched_directory(spec: &Path) -> PathBuf {
    match spec.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Blocks until the spec changes, then drains the burst until [`DEBOUNCE`] passes without events.
/// Returns `false` when the watcher is gone.
fn wait_for_change(receiver: &Receiver<EventResult>, spec_name: &OsString, quiet: Duration) -> bool {
    loop {
        match receiver.recv() {
            Ok(event) if touches(&event, spec_name) => break,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
    loop {
        match receiver.recv_timeout(quiet) {
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => return true,
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

/// Compares file names only: watcher paths may be canonicalized (`/private/var` on macOS).
fn touches(event: &EventResult, spec_name: &OsString) -> bool {
    let Ok(event) = event else { return false };
    !matches!(event.kind, EventKind::Access(_))
        && event.paths.iter().any(|path| path.file_name() == Some(spec_name.as_os_str()))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Sender;
    use std::thread;

    use notify::event::{AccessKind, ModifyKind};

    use super::*;

    fn send(sender: &Sender<EventResult>, kind: EventKind, path: &str) {
        let _ = sender.send(Ok(Event::new(kind).add_path(PathBuf::from(path))));
    }

    #[test]
    fn a_burst_of_events_is_one_change() {
        let (sender, receiver) = mpsc::channel();
        let name = OsString::from("a.diorama.yaml");
        send(&sender, EventKind::Modify(ModifyKind::Any), "/x/other.png");
        send(&sender, EventKind::Access(AccessKind::Any), "/x/a.diorama.yaml");
        for _ in 0..5 {
            send(&sender, EventKind::Modify(ModifyKind::Any), "/x/a.diorama.yaml");
        }
        assert!(wait_for_change(&receiver, &name, Duration::from_millis(20)));
        assert!(receiver.try_recv().is_err(), "the burst must be drained");
    }

    #[test]
    fn closed_watcher_stops_the_loop() {
        let (sender, receiver) = mpsc::channel();
        let name = OsString::from("a.diorama.yaml");
        let handle = thread::spawn(move || {
            send(&sender, EventKind::Modify(ModifyKind::Any), "/x/unrelated.txt");
        });
        let _ = handle.join();
        assert!(!wait_for_change(&receiver, &name, Duration::from_millis(20)));
    }

    #[test]
    fn spec_in_current_directory_watches_dot() {
        assert_eq!(watched_directory(Path::new("a.yaml")), PathBuf::from("."));
        assert_eq!(watched_directory(Path::new("d/a.yaml")), PathBuf::from("d"));
    }
}
