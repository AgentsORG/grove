//! Running a user's command (a check, a bisect test) the way their shell
//! would: `sh -c` on Unix, `cmd /C` on Windows.

use std::path::Path;
use std::process::{Command, Output};

pub fn run(cmd: &str, dir: &Path) -> std::io::Result<Output> {
    #[cfg(windows)]
    {
        // cmd.exe parses its own command line, so the command must reach it
        // exactly as written; Rust's default quoting would escape inner
        // quotes in a way cmd doesn't understand.
        use std::os::windows::process::CommandExt;
        Command::new("cmd")
            .arg("/C")
            .raw_arg(cmd)
            .current_dir(dir)
            .output()
    }
    #[cfg(not(windows))]
    {
        Command::new("sh")
            .args(["-c", cmd])
            .current_dir(dir)
            .output()
    }
}
