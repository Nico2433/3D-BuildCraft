//! Running programs on the host when the app is sandboxed in a Flatpak.

use std::ffi::OsStr;
use std::process::Command;

/// Whether the app runs inside a Flatpak sandbox, so host programs must run outside it.
/// The user's home folder; the Flatpak sandbox shares the host's.
pub fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(Into::into)
}

pub fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists() || std::env::var_os("FLATPAK_ID").is_some()
}

/// A command for a host program: through `flatpak-spawn --host` inside the sandbox.
pub fn host_command(program: impl AsRef<OsStr>) -> Command {
    if in_flatpak() {
        let mut command = Command::new("flatpak-spawn");
        command.args(["--host"]).arg(program);
        command
    } else {
        Command::new(program)
    }
}
