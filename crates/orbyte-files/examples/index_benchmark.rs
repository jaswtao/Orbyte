//! Repeatable index benchmark. Author: jaswtao · Date: 2026-10-24
use orbyte_files::watcher::FileManager;
use std::{fs, time::{Duration, Instant}};

fn main() {
    let count: usize = std::env::args().nth(1).unwrap_or("10000".into()).parse().unwrap();
    assert!(count < 100_000);
    let root = std::env::temp_dir().join(format!("orbyte-index-bench-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join(".git/info")).unwrap();
    for i in 0..count { fs::write(root.join(format!("sample-{i:05}.txt")), b"baseline").unwrap(); }
    let manager = FileManager::default();
    let project = manager.project("benchmark", &root).unwrap();
    let start = Instant::now(); project.search("sample").unwrap();
    println!("files={count} first_ms={:.3}", start.elapsed().as_secs_f64()*1000.0);
    for scenario in ["repeat", "content", "git", "save", "create"] {
        let before = project.rebuild_count();
        let mut times = Vec::new();
        for i in 0..5 {
            match scenario {
                "content" => fs::write(root.join("sample-00000.txt"), format!("edit {i}")).unwrap(),
                "git" => fs::write(root.join(".git/index"), format!("index {i}")).unwrap(),
                "save" => {
                    let file = orbyte_files::ops::read(&root, "sample-00000.txt", None).unwrap();
                    orbyte_files::ops::write(&root, "sample-00000.txt", orbyte_files::ops::WriteRequest {
                        content: format!("save {i}"), expected_version: Some(file.version), encoding: file.encoding,
                        bom: file.bom, line_ending: file.line_ending,
                    }).unwrap();
                }
                "create" => fs::write(root.join(format!("added-{i}.txt")), b"new").unwrap(),
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(700));
            let start = Instant::now(); project.search("sample").unwrap();
            times.push(start.elapsed().as_secs_f64()*1000.0);
        }
        println!("scenario={scenario} search_ms={times:?} rebuilds={}", project.rebuild_count()-before);
    }
    drop(project); manager.clear();
    std::thread::sleep(Duration::from_millis(400));
    fs::remove_dir_all(root).unwrap();
}
