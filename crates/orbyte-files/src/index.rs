//! Filename index with optional ignore filtering. Author: jaswtao · Date: 2026-10-13
use crate::{path_guard::resolve, FileResult};
use serde::Serialize;
use std::{collections::HashSet, path::Path};

#[derive(Default)]
pub struct FileIndex {
    paths: Vec<String>,
    membership: HashSet<String>,
    directories: HashSet<String>,
    incomplete: bool,
}
#[derive(Serialize)]
pub struct SearchResult {
    pub paths: Vec<String>,
    pub incomplete: bool,
}
impl FileIndex {
    pub fn build(root: &Path) -> FileResult<Self> {
        Self::build_with_ignored(root, false)
    }

    pub fn build_with_ignored(root: &Path, include_ignored: bool) -> FileResult<Self> {
        let root = resolve(root, "", true)?;
        let mut index = Self::default();
        let walker = ignore::WalkBuilder::new(&root)
            .standard_filters(!include_ignored)
            .hidden(false)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|entry| {
                !excluded_path(&entry.file_name().to_string_lossy())
            })
            .build();
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    index.incomplete = true;
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if index.paths.len() == 100_000 {
                index.incomplete = true;
                break;
            }
            let path = entry
                .path()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // 遍历不跟随链接，条目均位于规范化根目录内，只需校验路径语法。
            if orbyte_core::validate_relative_path(&path).is_ok() {
                index.membership.insert(path.to_lowercase());
                for (offset, _) in path.match_indices('/') {
                    index.directories.insert(path[..offset].to_lowercase());
                }
                index.paths.push(path);
            }
        }
        Ok(index)
    }
    pub fn contains(&self, path: &str) -> bool { self.membership.contains(&path.to_lowercase()) }
    pub fn contains_directory(&self, path: &str) -> bool { self.directories.contains(&path.to_lowercase()) }

    pub fn search(&self, query: &str) -> SearchResult {
        let query = query.to_lowercase();
        let mut matches: Vec<_> = self
            .paths
            .iter()
            .filter_map(|path| {
                let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
                let mut needle = query.chars();
                let mut next = needle.next();
                for character in name.chars() {
                    if Some(character) == next {
                        next = needle.next();
                    }
                }
                next.is_none().then_some(path.clone())
            })
            .collect();
        matches.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        matches.truncate(50);
        SearchResult {
            paths: matches,
            incomplete: self.incomplete,
        }
    }
}

fn excluded_path(name: &str) -> bool { name.eq_ignore_ascii_case(".git") || (name.starts_with(".orbyte-") && name.ends_with(".tmp")) }
