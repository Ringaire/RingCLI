use std::path::PathBuf;

/// Base directory for all neko data: ~/.neko/
fn neko_home() -> PathBuf {
    std::env::var("NEKO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".neko"))
}

/// Legacy XDG paths for migration
mod legacy {
    use std::path::PathBuf;

    pub fn xdg_config() -> PathBuf {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".config"))
    }

    pub fn xdg_data() -> PathBuf {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".local").join("share"))
    }

    pub fn xdg_cache() -> PathBuf {
        std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".cache"))
    }

    pub fn xdg_state() -> PathBuf {
        std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".local").join("state"))
    }

    pub const APP: &str = "neko";
}

/// ~/.neko/config/settings.jsonc
pub fn config_path() -> PathBuf {
    std::env::var("NEKO_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| neko_home().join("config").join("settings.jsonc"))
}

/// ~/.neko/config/
pub fn config_dir() -> PathBuf {
    neko_home().join("config")
}

/// ~/.neko/sessions/
pub fn sessions_dir() -> PathBuf {
    neko_home().join("sessions")
}

/// ~/.neko/cache/
pub fn cache_dir() -> PathBuf {
    neko_home().join("cache")
}

/// ~/.neko/logs/
pub fn logs_dir() -> PathBuf {
    neko_home().join("logs")
}

/// ~/.neko/logs/neko.log
pub fn log_path() -> PathBuf {
    logs_dir().join("neko.log")
}

/// ~/.neko/history
pub fn history_path() -> PathBuf {
    neko_home().join("history")
}

/// ~/.neko/skills/
pub fn skills_dir() -> PathBuf {
    neko_home().join("skills")
}

/// Migration logic: move data from old XDG paths to new unified ~/.neko/
/// This function is called once during init_dirs() to ensure smooth transition.
pub fn migrate_from_xdg() -> Result<(), std::io::Error> {
    use std::fs;

    // Skip migration if NEKO_HOME is explicitly set (user overrides)
    if std::env::var("NEKO_HOME").is_ok() {
        return Ok(());
    }

    let home = neko_home();

    // Skip if new structure already exists
    if home.exists() && home.join("config").exists() {
        return Ok(());
    }

    // Define migration mapping: (old_path, new_path, description)
    let migrations = vec![
        (
            legacy::xdg_config().join(legacy::APP),
            config_dir(),
            "config",
        ),
        (
            legacy::xdg_data().join(legacy::APP).join("sessions"),
            sessions_dir(),
            "sessions",
        ),
        (
            legacy::xdg_data().join(legacy::APP).join("skills"),
            skills_dir(),
            "skills",
        ),
        (
            legacy::xdg_cache().join(legacy::APP),
            cache_dir(),
            "cache",
        ),
        (
            legacy::xdg_state().join(legacy::APP),
            logs_dir(),
            "logs",
        ),
    ];

    let mut migrated_any = false;

    for (old_path, new_path, name) in migrations {
        if old_path.exists() {
            // Ensure parent directory exists
            if let Some(parent) = new_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Handle potential conflicts: if new_path exists, skip migration
            if new_path.exists() {
                eprintln!(
                    "[neko] Warning: {} already exists at {}, skipping migration from {}",
                    name,
                    new_path.display(),
                    old_path.display()
                );
                continue;
            }

            eprintln!(
                "[neko] Migrating {} from {} to {}",
                name,
                old_path.display(),
                new_path.display()
            );

            // Move the directory
            match fs::rename(&old_path, &new_path) {
                Ok(_) => {
                    eprintln!("[neko] ✓ Migrated {}", name);
                    migrated_any = true;
                }
                Err(e) => {
                    eprintln!(
                        "[neko] Warning: Failed to migrate {}: {}. Manual intervention may be required.",
                        name, e
                    );
                    // Don't fail the entire migration if one path fails
                }
            }
        }
    }

    // Special case: migrate history file separately (it's a file, not a directory)
    let old_history = legacy::xdg_state().join(legacy::APP).join("history");
    let new_history = history_path();
    if old_history.exists() && !new_history.exists() {
        if let Some(parent) = new_history.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::rename(&old_history, &new_history) {
            Ok(_) => {
                eprintln!("[neko] ✓ Migrated history");
                migrated_any = true;
            }
            Err(e) => {
                eprintln!("[neko] Warning: Failed to migrate history: {}", e);
            }
        }
    }

    // Clean up empty old directories (best effort, ignore errors)
    if migrated_any {
        let _ = fs::remove_dir(legacy::xdg_state().join(legacy::APP));
        let _ = fs::remove_dir(legacy::xdg_data().join(legacy::APP));
        let _ = fs::remove_dir(legacy::xdg_config().join(legacy::APP));
        eprintln!("[neko] Migration complete. All data is now in {}", home.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neko_home_default() {
        // Without NEKO_HOME env var, should return ~/.neko
        std::env::remove_var("NEKO_HOME");
        let home = neko_home();
        assert!(home.ends_with(".neko"));
    }

    #[test]
    fn test_neko_home_override() {
        // With NEKO_HOME env var, should use that path
        let custom_path = "/tmp/custom_neko";
        std::env::set_var("NEKO_HOME", custom_path);
        let home = neko_home();
        assert_eq!(home, PathBuf::from(custom_path));
        std::env::remove_var("NEKO_HOME");
    }

    #[test]
    fn test_paths_structure() {
        std::env::remove_var("NEKO_HOME");
        let home = neko_home();

        assert_eq!(config_dir(), home.join("config"));
        assert_eq!(config_path(), home.join("config").join("settings.jsonc"));
        assert_eq!(sessions_dir(), home.join("sessions"));
        assert_eq!(cache_dir(), home.join("cache"));
        assert_eq!(logs_dir(), home.join("logs"));
        assert_eq!(log_path(), home.join("logs").join("neko.log"));
        assert_eq!(history_path(), home.join("history"));
        assert_eq!(skills_dir(), home.join("skills"));
    }
}
