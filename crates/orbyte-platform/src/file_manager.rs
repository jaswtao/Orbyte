//! Open project paths in the system file manager. Author: jaswtao · Date: 2026-10-30
use std::{io, path::Path, process::Command};

fn command(path: &Path, directory: bool) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("explorer.exe");
        if directory {
            command.arg(path);
        } else {
            let mut argument = std::ffi::OsString::from("/select,");
            argument.push(path);
            command.arg(argument);
        }
        command
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        if !directory {
            command.arg("-R");
        }
        command.arg(path);
        command
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(if directory {
            path
        } else {
            path.parent().unwrap_or(path)
        });
        command
    }
}

pub fn open(path: &Path) -> io::Result<()> {
    let metadata = path.metadata()?;
    command(path, metadata.is_dir()).spawn()?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn explorer_opens_directories_and_selects_files_as_one_argument() {
        let path = Path::new(r"C:\project space\中文, file.txt");
        let file = command(path, false);
        assert_eq!(file.get_program(), "explorer.exe");
        assert_eq!(
            file.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new(r"/select,C:\project space\中文, file.txt")]
        );
        let directory = command(path.parent().unwrap(), true);
        assert_eq!(
            directory.get_args().collect::<Vec<_>>(),
            vec![path.parent().unwrap().as_os_str()]
        );
    }

    #[test]
    fn missing_paths_fail_before_starting_explorer() {
        let path = std::env::temp_dir()
            .join(format!("orbyte-missing-{}", std::process::id()))
            .join("missing.txt");
        assert_eq!(open(&path).unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}
