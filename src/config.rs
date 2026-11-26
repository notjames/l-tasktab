//! XDG config load/save for `$XDG_CONFIG_HOME/l-tasktabs/config.toml`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const QUALIFIER:    &str = "com";
const ORGANIZATION: &str = "jimconn";
const APPLICATION:  &str = "l-tasktabs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ElevationMode
{
  AskEveryTime,
  SudoTicket,
  #[default]
  AskOnceMemory,
  Keyring,
}

impl ElevationMode
{
  pub fn as_str(&self) -> &'static str
  {
    match self
    {
      Self::AskEveryTime  => "ask_every_time",
      Self::SudoTicket    => "sudo_ticket",
      Self::AskOnceMemory => "ask_once_memory",
      Self::Keyring       => "keyring",
    }
  }

  pub fn parse(s: &str) -> Option<Self>
  {
    match s
    {
      "ask_every_time"  => Some(Self::AskEveryTime),
      "sudo_ticket"     => Some(Self::SudoTicket),
      "ask_once_memory" => Some(Self::AskOnceMemory),
      "keyring"         => Some(Self::Keyring),
      _                 => None,
    }
  }

  pub fn all_labels() -> &'static [(&'static str, &'static str)]
  {
    &[
      ("ask_once_memory", "Ask once (memory)"),
      ("ask_every_time",  "Ask every time"),
      ("sudo_ticket",     "Reuse sudo ticket"),
      ("keyring",         "Keyring (optional)"),
    ]
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings
{
  #[serde(default)]
  pub autostart:      bool,
  #[serde(default)]
  pub elevation_mode: ElevationMode,
  #[serde(default = "default_true")]
  pub hide_on_close:  bool,
}

fn default_true() -> bool
{
  true
}

impl Default for AppSettings
{
  fn default() -> Self
  {
    Self {
      autostart:      false,
      elevation_mode: ElevationMode::AskOnceMemory,
      hide_on_close:  true,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig
{
  pub id:                 String,
  pub name:               String,
  pub command:            String,
  #[serde(default = "default_cwd")]
  pub working_directory:  String,
  #[serde(default)]
  pub autostart:          bool,
  #[serde(default)]
  pub elevated:           bool,
  #[serde(default)]
  pub backend:            SessionBackend,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub emulator:           Option<String>,
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub env:                BTreeMap<String, String>,
}

fn default_cwd() -> String
{
  "~".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionBackend
{
  #[default]
  Builtin,
  External,
}

impl Default for SessionConfig
{
  fn default() -> Self
  {
    Self {
      id:                String::new(),
      name:              String::new(),
      command:           String::new(),
      working_directory: default_cwd(),
      autostart:         false,
      elevated:          false,
      backend:           SessionBackend::Builtin,
      emulator:          None,
      env:               BTreeMap::new(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config
{
  #[serde(default)]
  pub app:      AppSettings,
  #[serde(default)]
  pub sessions: Vec<SessionConfig>,
}

impl Config
{
  pub fn project_dirs() -> Option<ProjectDirs>
  {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
  }

  pub fn config_path() -> Option<PathBuf>
  {
    Self::project_dirs().map(|p| p.config_dir().join("config.toml"))
  }

  pub fn load_or_create() -> Result<(Self, PathBuf), ConfigError>
  {
    let path = Self::config_path().ok_or(ConfigError::NoXdg)?;
    path.parent().map(fs::create_dir_all).transpose()?;

    match path.exists()
    {
      true  => Ok((Self::load_from(&path)?, path)),
      false =>
      {
        let cfg = Self::default();
        cfg.save_to(&path)?;
        Ok((cfg, path))
      }
    }
  }

  pub fn load_from(path: &Path) -> Result<Self, ConfigError>
  {
    let text = fs::read_to_string(path)?;
    let cfg: Self = toml::from_str(&text)?;
    Ok(cfg)
  }

  pub fn save_to(&self, path: &Path) -> Result<(), ConfigError>
  {
    path.parent().map(fs::create_dir_all).transpose()?;
    let text   = toml::to_string_pretty(self)?;
    let header = "# L-TaskTabs configuration\n# See PLAN.md for schema notes.\n\n";
    fs::write(path, format!("{header}{text}"))?;
    Ok(())
  }

  pub fn upsert_session(&mut self, mut session: SessionConfig) -> Result<(), String>
  {
    session.name              = session.name.trim().to_string();
    session.command           = session.command.trim().to_string();
    session.working_directory = session.working_directory.trim().to_string();

    match session.working_directory.is_empty()
    {
      true  => session.working_directory = default_cwd(),
      false => {}
    }

    match session.name.is_empty()
    {
      true  => return Err("Session name is required".into()),
      false => {}
    }
    // Empty command = shell-only session (short PS1).

    // Stable key for config/tray; owner never sets this — UUID on create.
    session.id = match session.id.trim().is_empty()
    {
      true  => Uuid::new_v4().to_string(),
      false => session.id.trim().to_string(),
    };

    match self.sessions.iter_mut().find(|s| s.id == session.id)
    {
      Some(existing) => *existing = session,
      None           => self.sessions.push(session),
    }
    Ok(())
  }

  pub fn remove_session(&mut self, id: &str) -> bool
  {
    let before = self.sessions.len();
    self.sessions.retain(|s| s.id != id);
    self.sessions.len() != before
  }
}

#[derive(Debug)]
pub enum ConfigError
{
  NoXdg,
  Io(io::Error),
  TomlDe(toml::de::Error),
  TomlSer(toml::ser::Error),
}

impl std::fmt::Display for ConfigError
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
  {
    match self
    {
      Self::NoXdg      => write!(f, "could not resolve XDG config directory"),
      Self::Io(e)      => write!(f, "{e}"),
      Self::TomlDe(e)  => write!(f, "invalid config.toml: {e}"),
      Self::TomlSer(e) => write!(f, "serialize config: {e}"),
    }
  }
}

impl From<io::Error> for ConfigError
{
  fn from(value: io::Error) -> Self
  {
    Self::Io(value)
  }
}

impl From<toml::de::Error> for ConfigError
{
  fn from(value: toml::de::Error) -> Self
  {
    Self::TomlDe(value)
  }
}

impl From<toml::ser::Error> for ConfigError
{
  fn from(value: toml::ser::Error) -> Self
  {
    Self::TomlSer(value)
  }
}
