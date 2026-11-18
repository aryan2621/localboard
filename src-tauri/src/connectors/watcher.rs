use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::files::FileFormat;

/// Quiet period after the last file event before a refresh runs; editors write in bursts.
const DEBOUNCE: Duration = Duration::from_millis(750);
/// Upper bound on one burst so a steadily-written file still refreshes.
const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub enum WatchScope {
    File,
    Folder { recursive: bool },
}

#[derive(Clone, Debug)]
pub struct WatchTarget {
    pub connection_id: String,
    pub path: PathBuf,
    pub scope: WatchScope,
}

impl WatchTarget {
    fn matches(&self, changed: &std::path::Path) -> bool {
        match self.scope {
            WatchScope::File => changed == self.path,
            WatchScope::Folder { recursive } => {
                let inside = if recursive { changed.starts_with(&self.path) } else { changed.parent() == Some(self.path.as_path()) };
                inside && FileFormat::from_path(changed).is_some()
            }
        }
    }

    fn watch_root(&self) -> Option<(PathBuf, RecursiveMode)> {
        match self.scope {
            // Watch the parent: editors often replace files atomically, which drops a direct file watch.
            WatchScope::File => self.path.parent().map(|parent| (parent.to_path_buf(), RecursiveMode::NonRecursive)),
            WatchScope::Folder { recursive } => Some((
                self.path.clone(),
                if recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive },
            )),
        }
    }
}

/// Watches file and folder connections; dropping it stops the watch and its worker thread.
pub struct CatalogWatcher {
    _watcher: RecommendedWatcher,
}

/// `on_change` receives the ids of connections whose files changed, debounced.
pub fn start<F>(targets: Vec<WatchTarget>, on_change: F) -> notify::Result<CatalogWatcher>
where
    F: Fn(BTreeSet<String>) + Send + 'static,
{
    // Watch roots and targets are canonical so they compare equal to the (canonical) paths notify reports.
    let targets: Vec<WatchTarget> = targets
        .into_iter()
        .map(|mut target| {
            target.path = std::fs::canonicalize(&target.path).unwrap_or(target.path);
            target
        })
        .collect();
    let (sender, receiver) = mpsc::channel::<BTreeSet<String>>();
    let filter = targets.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
        Ok(event) => {
            // Filter here so unrelated events never wake or extend the debounce.
            let mut hit = BTreeSet::new();
            collect(&filter, &event, &mut hit);
            if !hit.is_empty() {
                let _ = sender.send(hit);
            }
        }
        Err(error) => tracing::warn!("file watcher error: {error}"),
    })?;

    let mut roots: BTreeMap<PathBuf, RecursiveMode> = BTreeMap::new();
    for (root, mode) in targets.iter().filter_map(WatchTarget::watch_root) {
        let entry = roots.entry(root).or_insert(mode);
        if mode == RecursiveMode::Recursive {
            *entry = RecursiveMode::Recursive;
        }
    }
    for (root, mode) in &roots {
        if let Err(error) = watcher.watch(root, *mode) {
            tracing::warn!("cannot watch {}: {error}", root.display());
        }
    }

    std::thread::Builder::new()
        .name("localboard-file-watcher".into())
        .spawn(move || {
            // Exits when the watcher (and with it the sender) is dropped.
            while let Ok(mut changed) = receiver.recv() {
                let started = std::time::Instant::now();
                while let Some(left) = MAX_WAIT.checked_sub(started.elapsed()) {
                    match receiver.recv_timeout(DEBOUNCE.min(left)) {
                        Ok(more) => changed.extend(more),
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                on_change(changed);
            }
        })
        .map_err(|error| notify::Error::generic(&error.to_string()))?;

    Ok(CatalogWatcher { _watcher: watcher })
}

fn collect(targets: &[WatchTarget], event: &notify::Event, changed: &mut BTreeSet<String>) {
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    for path in &event.paths {
        for target in targets.iter().filter(|target| target.matches(path)) {
            changed.insert(target.connection_id.clone());
        }
    }
}
