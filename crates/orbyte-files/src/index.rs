//! 遵守忽略规则的文件名索引。Author: jaswtao · Date: 2026-10-13
use crate::{path_guard::resolve, FileResult};
use serde::Serialize;
use std::path::Path;

#[derive(Default)]
pub struct FileIndex {
    paths: Vec<String>,
    incomplete: bool,
}
#[derive(Serialize)]
pub struct SearchResult {
    pub paths: Vec<String>,
    pub incomplete: bool,
}
impl FileIndex {
    pub fn build(root: &Path) -> FileResult<Self> {
        let root = resolve(root, "", true)?;
        let mut index = Self::default();
        let walker = ignore::WalkBuilder::new(&root)
            .hidden(false)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|entry| {
                !entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(".git")
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
                index.paths.push(path);
            }
        }
        Ok(index)
    }
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
