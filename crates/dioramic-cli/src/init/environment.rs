//! Where `init` reads and writes: the project, the home directory, per-OS app folders and `PATH`.
//!
//! Everything comes from an [`Environment`] value so tests can point `init` at temporary
//! directories and never touch the real user files.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Operating system family, for the per-OS config locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS (`~/Library/Application Support`).
    MacOs,
    /// Windows (`%APPDATA%`).
    Windows,
    /// Linux and other Unix (`$XDG_CONFIG_HOME`).
    Linux,
}

impl Platform {
    /// The platform this binary was built for.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Injected paths and lookups used by `init`.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Project root (`--dir`, else the current directory).
    pub project_dir: PathBuf,
    /// User home (`HOME` / `USERPROFILE`).
    pub home_dir: Option<PathBuf>,
    /// `%APPDATA%` (Windows only).
    pub app_data_dir: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, else `~/.config` (Linux only).
    pub config_dir: Option<PathBuf>,
    /// Absolute path of the running `dioramic` binary (written in user-scope configs).
    pub executable: Option<PathBuf>,
    /// Directories of `PATH`, used to detect `claude` and `code`.
    pub path_dirs: Vec<PathBuf>,
    /// OS family.
    pub platform: Platform,
}

impl Environment {
    /// Reads the real process environment.
    pub fn from_process(project_dir: PathBuf) -> Self {
        let home_dir = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
        let config_dir = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir.as_ref().map(|home| home.join(".config")));
        Self {
            project_dir,
            home_dir,
            app_data_dir: env::var_os("APPDATA").map(PathBuf::from),
            config_dir,
            executable: env::current_exe().ok().map(|path| path.canonicalize().unwrap_or(path)),
            path_dirs: env::var_os("PATH").map(|path| env::split_paths(&path).collect()).unwrap_or_default(),
            platform: Platform::current(),
        }
    }

    /// Finds an executable on `PATH` (`claude`, `code`).
    pub fn find_program(&self, name: &str) -> Option<PathBuf> {
        let candidates: Vec<OsString> = if self.platform == Platform::Windows {
            ["exe", "cmd", "bat"].iter().map(|extension| OsString::from(format!("{name}.{extension}"))).collect()
        } else {
            vec![OsString::from(name)]
        };
        self.path_dirs
            .iter()
            .flat_map(|directory| candidates.iter().map(move |candidate| directory.join(candidate)))
            .find(|path| path.is_file())
    }

    /// Path inside the project.
    pub fn project_path(&self, relative: &str) -> PathBuf {
        self.project_dir.join(relative)
    }

    /// Path inside the home directory.
    pub fn home_path(&self, relative: &str) -> Option<PathBuf> {
        self.home_dir.as_ref().map(|home| home.join(relative))
    }

    /// `~/Library/Application Support` (macOS), `%APPDATA%` (Windows), `$XDG_CONFIG_HOME` (Linux).
    fn app_config_dir(&self) -> Option<PathBuf> {
        match self.platform {
            Platform::MacOs => self.home_path("Library/Application Support"),
            Platform::Windows => self.app_data_dir.clone(),
            Platform::Linux => self.config_dir.clone(),
        }
    }

    /// VS Code user `mcp.json` (`research/clients.md` §2.1; default profile, stable channel).
    pub fn vscode_user_mcp(&self) -> Option<PathBuf> {
        self.app_config_dir().map(|directory| directory.join("Code").join("User").join("mcp.json"))
    }

    /// Claude Desktop config; `None` on Linux, where the app does not exist (`research/clients.md` §3).
    pub fn claude_desktop_config(&self) -> Option<PathBuf> {
        match self.platform {
            Platform::Linux => None,
            Platform::MacOs | Platform::Windows => {
                self.app_config_dir().map(|directory| directory.join("Claude").join("claude_desktop_config.json"))
            }
        }
    }

    /// The project directory, `~/…` when it is inside the home.
    pub fn display_project(&self) -> String {
        match self.home_dir.as_ref().and_then(|home| self.project_dir.strip_prefix(home).ok()) {
            Some(relative) => format!("~/{}", relative.display()),
            None => self.project_dir.display().to_string(),
        }
    }

    /// Short display form: relative to the project, `~/…` in the home, else absolute.
    pub fn display(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.project_dir) {
            return relative.display().to_string();
        }
        if let Some(relative) = self.home_dir.as_ref().and_then(|home| path.strip_prefix(home).ok()) {
            return format!("~/{}", relative.display());
        }
        path.display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(platform: Platform) -> Environment {
        Environment {
            project_dir: PathBuf::from("/work/app"),
            home_dir: Some(PathBuf::from("/home/me")),
            app_data_dir: Some(PathBuf::from("C:/Users/me/AppData/Roaming")),
            config_dir: Some(PathBuf::from("/home/me/.config")),
            executable: None,
            path_dirs: Vec::new(),
            platform,
        }
    }

    #[test]
    fn per_os_locations() {
        let mac = environment(Platform::MacOs);
        assert_eq!(
            mac.claude_desktop_config(),
            Some(PathBuf::from("/home/me/Library/Application Support/Claude/claude_desktop_config.json"))
        );
        assert_eq!(
            mac.vscode_user_mcp(),
            Some(PathBuf::from("/home/me/Library/Application Support/Code/User/mcp.json"))
        );
        let linux = environment(Platform::Linux);
        assert_eq!(linux.claude_desktop_config(), None);
        assert_eq!(linux.vscode_user_mcp(), Some(PathBuf::from("/home/me/.config/Code/User/mcp.json")));
        let windows = environment(Platform::Windows);
        assert!(windows.claude_desktop_config().is_some_and(|path| path.starts_with("C:/Users/me/AppData/Roaming")));
    }

    #[test]
    fn display_is_short() {
        let environment = environment(Platform::Linux);
        assert_eq!(environment.display(Path::new("/work/app/.mcp.json")), ".mcp.json");
        assert_eq!(environment.display(Path::new("/home/me/.claude.json")), "~/.claude.json");
        assert_eq!(environment.display(Path::new("/etc/x")), "/etc/x");
    }
}
