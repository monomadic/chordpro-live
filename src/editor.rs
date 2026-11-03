//! External editing, following chordpro-tui/internal/tui/model.go's `e` binding.
//! Terminal supplies the TTY that a Finder-launched viewer does not have.
use std::{io, path::Path, process::Command};

fn command(editor: &str, path: &Path) -> io::Result<String> {
    let path = path.canonicalize()?;
    let path = path.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "song path is not valid UTF-8")
    })?;
    let editor = if editor.trim().is_empty() {
        "vi"
    } else {
        editor
    };
    // The editor is a user-provided shell command; the filename is always data.
    Ok(format!("{editor} '{}'", path.replace('\'', "'\\''")))
}

pub fn open(path: &Path) -> io::Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_default();
    launch(&command(&editor, path)?)
}

fn launch(command: &str) -> io::Result<()> {
    let output = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv\n tell application \"Terminal\"\n activate\n do script (item 1 of argv)\n end tell\nend run",
            "--",
            command,
        ])
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_is_one_argument_even_with_shell_characters() {
        let dir = std::env::temp_dir().join(format!("cpl-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a 'song' $(false); [C].cho");
        std::fs::write(&path, "[C]Hello").unwrap();
        let command = command("printf '%s'", &path).unwrap();
        let result = Command::new("/bin/sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            path.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(super::command("  ", &path).unwrap().starts_with("vi "));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "opens Terminal; run explicitly for the macOS editor smoke check"]
    fn terminal_runs_the_editor_with_the_song_path() {
        let path =
            std::env::temp_dir().join(format!("cpl-editor-smoke-{}.cho", std::process::id()));
        let receipt = path.with_extension("receipt");
        std::fs::write(&path, "[C]Hello").unwrap();
        let editor = format!("printf '%s' > '{}'", receipt.display());
        launch(&command(&editor, &path).unwrap()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !receipt.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(
            std::fs::read_to_string(&receipt).unwrap(),
            path.canonicalize().unwrap().to_str().unwrap()
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(receipt).unwrap();
    }
}
