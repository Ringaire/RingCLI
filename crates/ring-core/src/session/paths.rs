use std::path::PathBuf;

/// Base directory for all ring data: ~/.ring/
fn ring_home() -> PathBuf {
    std::env::var("RING_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".ring"))
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

    pub const APP: &str = "ring";
}

/// ~/.ring/config/settings.jsonc
pub fn config_path() -> PathBuf {
    std::env::var("RING_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ring_home().join("config").join("settings.jsonc"))
}

/// ~/.ring/config/
pub fn config_dir() -> PathBuf {
    ring_home().join("config")
}

/// ~/.ring/sessions/
pub fn sessions_dir() -> PathBuf {
    ring_home().join("sessions")
}

/// ~/.ring/cache/
pub fn cache_dir() -> PathBuf {
    ring_home().join("cache")
}

/// ~/.ring/logs/
pub fn logs_dir() -> PathBuf {
    ring_home().join("logs")
}

/// ~/.ring/logs/ring.log
pub fn log_path() -> PathBuf {
    logs_dir().join("ring.log")
}

/// ~/.ring/history
pub fn history_path() -> PathBuf {
    ring_home().join("history")
}

/// ~/.ring/skills/
pub fn skills_dir() -> PathBuf {
    ring_home().join("skills")
}

/// ~/.ring/prompts/
pub fn prompts_dir() -> PathBuf {
    ring_home().join("prompts")
}

/// ~/.ring/doc/
pub fn doc_dir() -> PathBuf {
    ring_home().join("doc")
}

/// ~/.ring/mode/
pub fn mode_dir() -> PathBuf {
    ring_home().join("mode")
}

/// ~/.ring/config/tool.json
pub fn tool_json_path() -> PathBuf {
    config_dir().join("tool.json")
}

/// ~/.ring/config/mcp_server.json
pub fn mcp_server_json_path() -> PathBuf {
    config_dir().join("mcp_server.json")
}

/// Migration logic: move data from old XDG paths to new unified ~/.ring/
/// This function is called once during init_dirs() to ensure smooth transition.
pub fn migrate_from_xdg() -> Result<(), std::io::Error> {
    use std::fs;

    // Skip migration if RING_HOME is explicitly set (user overrides)
    if std::env::var("RING_HOME").is_ok() {
        return Ok(());
    }

    let home = ring_home();

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
                    "[ring] Warning: {} already exists at {}, skipping migration from {}",
                    name,
                    new_path.display(),
                    old_path.display()
                );
                continue;
            }

            eprintln!(
                "[ring] Migrating {} from {} to {}",
                name,
                old_path.display(),
                new_path.display()
            );

            // Move the directory
            match fs::rename(&old_path, &new_path) {
                Ok(_) => {
                    eprintln!("[ring] ✓ Migrated {}", name);
                    migrated_any = true;
                }
                Err(e) => {
                    eprintln!(
                        "[ring] Warning: Failed to migrate {}: {}. Manual intervention may be required.",
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
                eprintln!("[ring] ✓ Migrated history");
                migrated_any = true;
            }
            Err(e) => {
                eprintln!("[ring] Warning: Failed to migrate history: {}", e);
            }
        }
    }

    // Clean up empty old directories (best effort, ignore errors)
    if migrated_any {
        let _ = fs::remove_dir(legacy::xdg_state().join(legacy::APP));
        let _ = fs::remove_dir(legacy::xdg_data().join(legacy::APP));
        let _ = fs::remove_dir(legacy::xdg_config().join(legacy::APP));
        eprintln!("[ring] Migration complete. All data is now in {}", home.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_home_default() {
        // Without RING_HOME env var, should return ~/.ring
        std::env::remove_var("RING_HOME");
        let home = ring_home();
        assert!(home.ends_with(".ring"));
    }

    #[test]
    fn test_ring_home_override() {
        // With RING_HOME env var, should use that path
        let custom_path = "/tmp/custom_ring";
        std::env::set_var("RING_HOME", custom_path);
        let home = ring_home();
        assert_eq!(home, PathBuf::from(custom_path));
        std::env::remove_var("RING_HOME");
    }

    #[test]
    fn test_paths_structure() {
        std::env::remove_var("RING_HOME");
        let home = ring_home();

        assert_eq!(config_dir(), home.join("config"));
        assert_eq!(config_path(), home.join("config").join("settings.jsonc"));
        assert_eq!(sessions_dir(), home.join("sessions"));
        assert_eq!(cache_dir(), home.join("cache"));
        assert_eq!(logs_dir(), home.join("logs"));
        assert_eq!(log_path(), home.join("logs").join("ring.log"));
        assert_eq!(history_path(), home.join("history"));
        assert_eq!(skills_dir(), home.join("skills"));
        assert_eq!(prompts_dir(), home.join("prompts"));
        assert_eq!(mode_dir(), home.join("mode"));
        assert_eq!(tool_json_path(), home.join("config").join("tool.json"));
        assert_eq!(mcp_server_json_path(), home.join("config").join("mcp_server.json"));
    }
}
