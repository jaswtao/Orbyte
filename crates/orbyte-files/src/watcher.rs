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
        atomic::{AtomicBool, Ordering},
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
    index: Mutex<Option<FileIndex>>,
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
    pub fn search(&self, query: &str) -> FileResult<SearchResult> {
        let mut index = self.index.lock().unwrap();
        if self.dirty.swap(false, Ordering::AcqRel) || index.is_none() {
            match FileIndex::build(&self.root) {
                Ok(next) => *index = Some(next),
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
        let (mark, full) = (dirty.clone(), overflow.clone());
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                mark.store(true, Ordering::Release);
                if tx.try_send(event).is_err() {
                    full.store(true, Ordering::Release);
                }
            })
            .map_err(|e| format!("无法监听项目文件：{e}"))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| format!("无法监听项目文件：{e}"))?;
        let (thread_root, thread_sink, thread_stop) =
            (root.to_path_buf(), sink.clone(), stopped.clone());
        std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                let first = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => event,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let deadline = Instant::now() + Duration::from_millis(300);
                let mut paths = BTreeSet::new();
                let mut rescan = overflow.swap(false, Ordering::AcqRel);
                let mut collect = |event: notify::Result<notify::Event>| match event {
                    Ok(event) => {
                        rescan |= event.need_rescan();
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
            index: Mutex::new(None),
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
