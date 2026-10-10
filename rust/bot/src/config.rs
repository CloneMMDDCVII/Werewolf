//! `werewolf.toml`: the bot's only configuration file. The token can come
//! from `WEREWOLF_BOT_TOKEN` instead, which wins over the file, so a
//! container or service manager never has to write a secret to disk.

use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const TOKEN_ENV: &str = "WEREWOLF_BOT_TOKEN";
pub const FILE_NAME: &str = "werewolf.toml";

pub const TEMPLATE: &str = r#"# Werewolf for Telegram: bot configuration.
# Every setting below is optional except the token. Relative paths are
# resolved from the folder this file is in.

# The bot token from @BotFather (https://t.me/BotFather): send it /newbot.
# Leave this commented out to pass it in the WEREWOLF_BOT_TOKEN environment
# variable instead; the environment variable wins when both are set.
# token = "123456789:AAAA-your-token-here"

# Where the bot keeps groups, players, bans and game history.
#   sqlite://werewolf.db          a file next to this config; nothing to install
#   postgres://user:pass@host/db  a Postgres server, for large deployments
# database = "sqlite://werewolf.db"

# Folder with the community language files (English.xml, Deutsch.xml, ...).
# English is built into the bot, so this folder is optional.
# languages_dir = "Languages"

# Language for groups that haven't picked one.
# default_language = "English"

# Telegram user ids allowed to run admin commands in any group.
# global_admins = [123456789]

# Telegram user ids allowed to check and install language files
# (/validatelangs, /uploadlang). Global admins can too.
# language_admins = [123456789]

# Rough memory one running game needs, used to refuse new games before the
# machine runs out of memory.
# per_game_memory_mb = 16
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub token: String,
    pub database: String,
    pub languages_dir: PathBuf,
    pub default_language: String,
    pub global_admins: Vec<i64>,
    pub language_admins: Vec<i64>,
    pub per_game_memory_mb: u64,
    /// The file this came from, if any.
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    token: Option<String>,
    database: Option<String>,
    languages_dir: Option<PathBuf>,
    default_language: Option<String>,
    global_admins: Option<Vec<i64>>,
    language_admins: Option<Vec<i64>>,
    per_game_memory_mb: Option<u64>,
}

#[derive(Debug)]
pub enum ConfigError {
    /// No config file and no token in the environment: a commented
    /// template was written here for the operator to fill in.
    TemplateWritten(PathBuf),
    /// The file exists but neither it nor the environment has a token.
    MissingToken(PathBuf),
    Unreadable(PathBuf, String),
    Invalid(PathBuf, String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::TemplateWritten(p) => write!(
                f,
                "No configuration found, so I created one at\n  {}\n\n\
                 Next step: get a bot token from @BotFather on Telegram (send it /newbot),\n\
                 then either put it in that file as  token = \"...\"\n\
                 or set the {TOKEN_ENV} environment variable, and run me again.",
                p.display()
            ),
            ConfigError::MissingToken(p) => write!(
                f,
                "No bot token. Add  token = \"...\"  to {} or set {TOKEN_ENV}.\n\
                 Get a token from @BotFather on Telegram (send it /newbot).",
                p.display()
            ),
            ConfigError::Unreadable(p, e) => write!(f, "could not read {}: {e}", p.display()),
            ConfigError::Invalid(p, e) => write!(f, "{} is not valid: {e}", p.display()),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The config file used when none is given: `werewolf.toml` next to the
/// executable, so double-clicking or running the binary from anywhere
/// finds the same file.
pub fn default_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(FILE_NAME)
}

/// Loads the configuration. `env_token` is the value of
/// `WEREWOLF_BOT_TOKEN`, passed in so tests don't touch the process
/// environment.
pub fn load(path: &Path, env_token: Option<String>) -> Result<Config, ConfigError> {
    let env_token = env_token.filter(|t| !t.trim().is_empty());
    let (file, base) = if path.exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::Unreadable(path.into(), e.to_string()))?;
        let file: FileConfig = toml::from_str(&raw).map_err(|e| ConfigError::Invalid(path.into(), e.to_string()))?;
        (file, Some(path.parent().unwrap_or(Path::new(".")).to_path_buf()))
    } else if env_token.is_some() {
        (FileConfig::default(), None)
    } else {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, TEMPLATE).map_err(|e| ConfigError::Unreadable(path.into(), e.to_string()))?;
        return Err(ConfigError::TemplateWritten(path.into()));
    };

    let token = env_token
        .or(file.token.filter(|t| !t.trim().is_empty()))
        .ok_or_else(|| ConfigError::MissingToken(path.into()))?;
    let base = base.unwrap_or_else(|| PathBuf::from("."));

    Ok(Config {
        token,
        database: resolve_database_url(file.database.as_deref().unwrap_or("sqlite://werewolf.db"), &base),
        languages_dir: resolve(&base, file.languages_dir.unwrap_or_else(|| PathBuf::from("Languages"))),
        default_language: file.default_language.unwrap_or_else(|| "English".into()),
        global_admins: file.global_admins.unwrap_or_default(),
        language_admins: file.language_admins.unwrap_or_default(),
        per_game_memory_mb: file.per_game_memory_mb.unwrap_or(16),
        path: path.exists().then(|| path.to_path_buf()),
    })
}

fn resolve(base: &Path, p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

/// A relative SQLite path is relative to the config file's folder, not to
/// wherever the bot happened to be started from.
fn resolve_database_url(url: &str, base: &Path) -> String {
    let Some(rest) = url.strip_prefix("sqlite://").or_else(|| url.strip_prefix("sqlite:")) else {
        return url.to_string();
    };
    if rest.is_empty() || rest.starts_with(":memory:") || Path::new(rest.split('?').next().unwrap_or("")).is_absolute() {
        return url.to_string();
    }
    format!("sqlite://{}", base.join(rest).display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("wolf-config-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn first_run_writes_a_commented_template_and_explains() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        let err = load(&path, None).unwrap_err();
        assert!(matches!(err, ConfigError::TemplateWritten(ref p) if p == &path));
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, TEMPLATE);
        // Every setting is present but commented out, so the template
        // itself parses and still has no token.
        assert!(toml::from_str::<FileConfig>(&written).is_ok());
        assert!(err.to_string().contains("@BotFather"));
        assert!(matches!(load(&path, None), Err(ConfigError::MissingToken(_))));
    }

    #[test]
    fn the_token_can_come_from_the_environment() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "database = \"sqlite://w.db\"\n").unwrap();
        let c = load(&path, Some("123:env".into())).unwrap();
        assert_eq!(c.token, "123:env");
    }

    #[test]
    fn no_file_but_an_environment_token_runs_on_defaults() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        let c = load(&path, Some("123:env".into())).unwrap();
        assert_eq!(c.database, "sqlite://./werewolf.db");
        assert!(!path.exists(), "no template is written when the bot can run");
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "token = \"123:file\"\n").unwrap();
        assert_eq!(load(&path, Some("123:env".into())).unwrap().token, "123:env");
        assert_eq!(load(&path, None).unwrap().token, "123:file");
    }

    #[test]
    fn relative_paths_resolve_from_the_config_folder() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "token = \"t\"\nlanguages_dir = \"langs\"\n").unwrap();
        let c = load(&path, None).unwrap();
        assert_eq!(c.database, format!("sqlite://{}", dir.join("werewolf.db").display()));
        assert_eq!(c.languages_dir, dir.join("langs"));
    }

    #[test]
    fn absolute_and_server_urls_are_left_alone() {
        let base = Path::new("/srv/wolf");
        assert_eq!(resolve_database_url("sqlite:///data/w.db", base), "sqlite:///data/w.db");
        assert_eq!(resolve_database_url("postgres://u@h/db", base), "postgres://u@h/db");
    }

    #[test]
    fn typos_in_the_file_are_reported() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "tokn = \"t\"\n").unwrap();
        assert!(matches!(load(&path, None), Err(ConfigError::Invalid(..))));
    }
}
