//! 有界事件队列与 300ms 去抖；订阅失联仍使索引过期。Author: jaswtao · Date: 2026-10-13
use crate::{
    index::{FileIndex, SearchResult},
    FileResult,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum FileChange {
    Paths { paths: Vec<String> },
    Rescan { rescan: bool },
}
type Sink = Arc<dyn Fn(FileChange) + Send + Sync>;
pub struct ProjectFiles {
    root: PathBuf,
    dirty: Arc<AtomicBool>,
    index: Arc<Mutex<Option<FileIndex>>>,
    rebuilds: AtomicU64,
    sink: Arc<Mutex<Option<Sink>>>,
    stopped: Arc<AtomicBool>,
    _watcher: RecommendedWatcher,
}
impl Drop for ProjectFiles {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}
impl ProjectFiles {
    pub fn rebuild_count(&self) -> u64 { self.rebuilds.load(Ordering::Relaxed) }

    pub fn search(&self, query: &str) -> FileResult<SearchResult> {
        self.search_with(query, || FileIndex::build(&self.root))
    }

    fn search_with(&self, query: &str, build: impl FnOnce() -> FileResult<FileIndex>) -> FileResult<SearchResult> {
        let mut index = self.index.lock().unwrap();
        if self.dirty.swap(false, Ordering::AcqRel) || index.is_none() {
            match build() {
                Ok(next) => { *index = Some(next); self.rebuilds.fetch_add(1, Ordering::Relaxed); },
                Err(error) => {
                    self.dirty.store(true, Ordering::Release);
                    return Err(error);
                }
            }
        }
        Ok(index.as_ref().unwrap().search(query))
    }
}
#[derive(Default)]
pub struct FileManager {
    projects: Mutex<HashMap<String, Arc<ProjectFiles>>>,
}
impl FileManager {
    /// 应用退出时停止所有项目监听；由后台清理线程调用。
    pub fn clear(&self) {
        self.projects.lock().unwrap().clear();
    }

    pub fn project(&self, id: &str, root: &Path) -> FileResult<Arc<ProjectFiles>> {
        let mut projects = self.projects.lock().unwrap();
        if let Some(project) = projects.get(id).filter(|p| p.root == root) {
            return Ok(project.clone());
        }
        let dirty = Arc::new(AtomicBool::new(true));
        let overflow = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let sink: Arc<Mutex<Option<Sink>>> = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::sync_channel(2048);
        let index = Arc::new(Mutex::new(None));
        let (mark, full) = (dirty.clone(), overflow.clone());
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if tx.try_send(event).is_err() {
                    full.store(true, Ordering::Release);
                    mark.store(true, Ordering::Release);
                }
            })
            .map_err(|e| format!("无法监听项目文件：{e}"))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| format!("无法监听项目文件：{e}"))?;
        let (thread_root, thread_sink, thread_stop) =
            (root.to_path_buf(), sink.clone(), stopped.clone());
        let (thread_dirty, thread_index) = (dirty.clone(), index.clone());
        std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                let first = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => event,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let deadline = Instant::now() + Duration::from_millis(300);
                let mut paths = BTreeSet::new();
                let mut events = Vec::new();
                let mut rescan = overflow.swap(false, Ordering::AcqRel);
                let mut collect = |event: notify::Result<notify::Event>| match event {
                    Ok(event) => {
                        rescan |= event.need_rescan();
                        if events.len() < 2048 { events.push(event.clone()); } else { rescan = true; }
                        for path in event.paths {
                            if let Ok(relative) = path.strip_prefix(&thread_root) {
                                let relative = relative.to_string_lossy().replace('\\', "/");
                                if !relative.split('/').any(|p| {
                                    p.eq_ignore_ascii_case(".git")
                                        || (p.starts_with(".orbyte-") && p.ends_with(".tmp"))
                                }) {
                                    paths.insert(relative);
                                }
                            } else {
                                rescan = true;
                            }
                        }
                        if paths.len() > 2048 {
                            paths.clear();
                            rescan = true;
                        }
                    }
                    Err(_) => rescan = true,
                };
                collect(first);
                while Instant::now() < deadline {
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(event) => collect(event),
                        Err(_) => break,
                    }
                }
                rescan |= overflow.swap(false, Ordering::AcqRel);
                let invalidate = match thread_index.try_lock() {
                    Ok(index) => invalidates_batch(&thread_root, &events,
                        |path| index.as_ref().is_some_and(|i: &FileIndex| i.contains(path)),
                        |path| index.as_ref().is_some_and(|i| i.contains_directory(path))),
                    // A build is in progress. Preserve structural events for the next search.
                    Err(_) => events.iter().any(|event| invalidates_index(&thread_root, event, |_| false)),
                };
                if rescan || invalidate { thread_dirty.store(true, Ordering::Release); }
                let callback = thread_sink.lock().unwrap().clone();
                if !thread_stop.load(Ordering::Acquire) {
                    if let Some(callback) = callback {
                        if rescan {
                            callback(FileChange::Rescan { rescan: true });
                        } else if !paths.is_empty() {
                            callback(FileChange::Paths {
                                paths: paths.into_iter().collect(),
                            });
                        }
                    }
                }
            }
        });
        let project = Arc::new(ProjectFiles {
            root: root.into(),
            dirty,
            index,
            rebuilds: AtomicU64::new(0),
            sink,
            stopped,
            _watcher: watcher,
        });
        projects.insert(id.into(), project.clone());
        Ok(project)
    }
    pub fn watch(
        &self,
        id: &str,
        root: &Path,
        callback: impl Fn(FileChange) + Send + Sync + 'static,
    ) -> FileResult<()> {
        *self.project(id, root)?.sink.lock().unwrap() = Some(Arc::new(callback));
        Ok(())
    }
    pub fn search(&self, id: &str, root: &Path, query: &str) -> FileResult<SearchResult> {
        self.project(id, root)?.search(query)
    }
    pub fn remove(&self, id: &str) {
        self.projects.lock().unwrap().remove(id);
    }
}


fn temporary(path: &str) -> bool {
    path.split('/').any(|p| p.starts_with(".orbyte-") && p.ends_with(".tmp"))
}

fn invalidates_index(root: &Path, event: &notify::Event, contains: impl Fn(&str) -> bool) -> bool {
    use notify::{EventKind, event::{ModifyKind, RenameMode}};
    if event.need_rescan() { return true; }
    if matches!(event.kind, EventKind::Access(_)) { return false; }
    if event.paths.is_empty() { return true; }
    event.paths.iter().any(|path| {
        let Ok(relative) = path.strip_prefix(root) else { return true; };
        let path = relative.to_string_lossy().replace('\\', "/");
        if temporary(&path) { return false; }
        let name = path.rsplit('/').next().unwrap_or("");
        if name.eq_ignore_ascii_case(".gitignore") || name.eq_ignore_ascii_case(".ignore") || path.eq_ignore_ascii_case(".git/info/exclude") { return true; }
        if path.split('/').any(|p| p.eq_ignore_ascii_case(".git")) { return false; }
        match event.kind {
            EventKind::Create(_) => !contains(&path),
            EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => false,
            // ReadDirectoryChangesW reports content and attribute writes as Modify(Any).
            EventKind::Modify(ModifyKind::Any) if cfg!(windows) => false,
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => !contains(&path),
            EventKind::Modify(ModifyKind::Name(_)) => {
                // A temporary source replaced an indexed destination: no filename changed.
                let atomic = event.paths.iter().any(|p| temporary(&p.to_string_lossy().replace('\\', "/")));
                !(atomic && contains(&path))
            }
            _ => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{Event, EventKind, event::{CreateKind, ModifyKind, DataChange, RenameMode, RemoveKind}};

    #[test]
    fn classification_preserves_structure_and_ignore_rules() {
        let root = Path::new("project");
        let check = |kind, paths: &[&str]| {
            let mut event = Event::new(kind);
            for path in paths { event = event.add_path(root.join(path)); }
            invalidates_index(root, &event, |p| p == "existing.txt")
        };
        let content = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        assert!(!check(content, &["existing.txt"]));
        assert!(!check(EventKind::Create(CreateKind::File), &["existing.txt"]));
        assert!(!check(content, &[".git/index"]));
        assert!(!check(EventKind::Remove(RemoveKind::File), &[".orbyte-save.tmp"]));
        assert!(!check(EventKind::Modify(ModifyKind::Name(RenameMode::Both)), &[".orbyte-save.tmp", "existing.txt"]));
        for path in [".gitignore", "nested/.gitignore", "nested/.ignore", ".git/info/exclude"] { assert!(check(content, &[path])); }
        assert!(check(EventKind::Create(CreateKind::File), &["new.txt"]));
        assert!(check(EventKind::Remove(RemoveKind::File), &["existing.txt"]));
        assert!(check(EventKind::Modify(ModifyKind::Name(RenameMode::Both)), &["existing.txt", "new.txt"]));
        assert!(check(EventKind::Other, &["existing.txt"]));
    }

    #[test]
    fn windows_replacement_batch_does_not_hide_a_real_delete() {
        let root = Path::new("project");
        let removed = Event::new(EventKind::Remove(RemoveKind::Any)).add_path(root.join("existing.txt"));
        let target = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To))).add_path(root.join("existing.txt"));
        assert!(!invalidates_batch(root, &[removed.clone(), target], |p| p == "existing.txt", |_| false));
        assert!(invalidates_batch(root, &[removed], |_| true, |_| false));
        let removed_ignore = Event::new(EventKind::Remove(RemoveKind::Any)).add_path(root.join(".gitignore"));
        let replaced_ignore = Event::new(EventKind::Create(CreateKind::Any)).add_path(root.join(".gitignore"));
        assert!(invalidates_batch(root, &[removed_ignore, replaced_ignore], |_| true, |_| false));
        let rescan = Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert!(invalidates_batch(root, &[rescan], |_| true, |_| false));
    }

    #[test]
    fn directory_replacement_preserves_descendant_invalidation() {
        let root = std::env::temp_dir().join(format!("orbyte-directory-batch-{}", uuid::Uuid::new_v4()));
        let directory = root.join("nested");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("old.txt"), "old").unwrap();
        let index = FileIndex::build(&root).unwrap();
        assert!(index.contains_directory("NESTED"));
        let removed = Event::new(EventKind::Remove(RemoveKind::Any)).add_path(directory.clone());
        let target = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To))).add_path(directory.clone());
        // Windows may report only the directory paths when a populated directory is moved.
        assert!(invalidates_batch(&root, &[removed.clone(), target.clone()], |p| index.contains(p), |p| index.contains_directory(p)));
        assert!(invalidates_batch(&root, &[removed.clone(), target], |_| false, |_| false));
        std::fs::remove_dir_all(&directory).unwrap();
        // Keep the old descendants invalidated even if the replacement was removed again.
        let created = Event::new(EventKind::Create(CreateKind::Any)).add_path(directory.clone());
        assert!(invalidates_batch(&root, &[removed, created], |p| index.contains(p), |p| index.contains_directory(p)));
        let removed = Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(directory.clone());
        let created = Event::new(EventKind::Create(CreateKind::Folder)).add_path(directory);
        assert!(invalidates_batch(&root, &[removed, created], |_| false, |_| false));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn event_during_build_survives_the_next_search() {
        let root = std::env::temp_dir().join(format!("orbyte-build-race-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let manager = FileManager::default();
        let project = manager.project("race", &root).unwrap();
        let first = project.search_with("during", || {
            let old_snapshot = FileIndex::build(&root)?;
            std::fs::write(root.join("during.txt"), "new file")?;
            // Inject the watcher signal after walking, while the build still holds the index lock.
            project.dirty.store(true, Ordering::Release);
            Ok(old_snapshot)
        }).unwrap();
        assert!(first.paths.is_empty());
        assert_eq!(project.search("during").unwrap().paths, ["during.txt"]);
        assert_eq!(project.rebuild_count(), 2);
        drop(project); manager.clear();
        std::fs::remove_dir_all(root).unwrap();
    }
}


fn invalidates_batch(root: &Path, events: &[notify::Event], contains: impl Fn(&str) -> bool, directory: impl Fn(&str) -> bool) -> bool {
    use notify::{EventKind, event::{CreateKind, ModifyKind, RemoveKind, RenameMode}};
    // ReplaceFileW also creates and removes a system backup and renames the old destination to it.
    // Compare the net filename changes in the batch, without special-casing OS backup names.
    let destinations: BTreeSet<_> = events.iter().filter(|event| matches!(event.kind,
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To | RenameMode::Both))))
        .filter_map(|event| event.paths.last()).collect();
    let removals: BTreeSet<_> = events.iter().filter(|event| matches!(event.kind, EventKind::Remove(_)))
        .flat_map(|event| event.paths.iter()).collect();
    events.iter().any(|event| {
        if event.need_rescan() { return true; }
        let mut filtered = event.clone();
        if matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))) {
            filtered.paths.retain(|p| {
                let Ok(relative) = p.strip_prefix(root) else { return true; };
                let relative = relative.to_string_lossy().replace('\\', "/");
                let name = relative.rsplit('/').next().unwrap_or("");
                if name.eq_ignore_ascii_case(".gitignore") || name.eq_ignore_ascii_case(".ignore") || relative.eq_ignore_ascii_case(".git/info/exclude") { return true; }
                // Directory replacement can change descendants even when its own name survives.
                if matches!(event.kind, EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder))
                    || directory(&relative) || p.is_dir() { return true; }
                if destinations.contains(p) && removals.contains(p) && !contains(&relative) { return false; }
                if matches!(event.kind, EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From)))
                    && destinations.contains(p) && contains(&relative) { return false; }
                true
            });
            if !event.paths.is_empty() && filtered.paths.is_empty() { return false; }
        }
        invalidates_index(root, &filtered, &contains)
    })
}
