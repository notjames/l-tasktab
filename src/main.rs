//! L-TaskTabs — QML main window, StatusNotifier tray, XDG config, single-instance.

mod config;

use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{LazyLock, Once};

use directories::BaseDirs;
use fs2::FileExt;
use image::GenericImageView;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use qtbridge::{QApp, QThreadHandle, QmlObject, qobject};
use serde_json::{json, Value as JsonValue};

use crate::config::{Config, ElevationMode, SessionConfig};

const APP_ID:   &str = "com.jimconn.ltasktabs";
const APP_NAME: &str = "L-TaskTabs";

struct InstanceLock
{
  _file: std::fs::File,
}

struct LttTray
{
  qt: QThreadHandle<AppController>,
}

fn load_tray_png(bytes: &'static [u8]) -> Option<ksni::Icon>
{
  let img              = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
  let (width, height)  = img.dimensions();
  let mut data         = img.into_rgba8().into_vec();
  for pixel in data.chunks_exact_mut(4)
  {
    pixel.rotate_right(1);
  }
  Some(ksni::Icon {
    width:  width as i32,
    height: height as i32,
    data,
  })
}

fn tray_icon_pixmaps() -> Vec<ksni::Icon>
{
  static ICONS: LazyLock<Vec<ksni::Icon>> = LazyLock::new(|| {
    [
      load_tray_png(include_bytes!("../assets/tray-64.png")),
      load_tray_png(include_bytes!("../assets/tray-32.png")),
    ]
    .into_iter()
    .flatten()
    .collect()
  });
  ICONS.clone()
}

impl ksni::Tray for LttTray
{
  fn id(&self) -> String
  {
    APP_ID.into()
  }

  fn title(&self) -> String
  {
    APP_NAME.into()
  }

  fn icon_name(&self) -> String
  {
    String::new()
  }

  fn icon_pixmap(&self) -> Vec<ksni::Icon>
  {
    tray_icon_pixmaps()
  }

  fn category(&self) -> ksni::Category
  {
    ksni::Category::ApplicationStatus
  }

  fn status(&self) -> ksni::Status
  {
    ksni::Status::Active
  }

  fn activate(&mut self, _x: i32, _y: i32)
  {
    self.qt.invoke_mut(AppController::show_main_window);
  }

  fn menu(&self) -> Vec<ksni::MenuItem<Self>>
  {
    vec![
      StandardItem {
        label:     "Show L-TaskTabs".into(),
        icon_name: "document-open".into(),
        activate:  Box::new(|this: &mut Self| {
          this.qt.invoke_mut(AppController::show_main_window);
        }),
        ..Default::default()
      }
      .into(),
      ksni::MenuItem::Separator,
      StandardItem {
        label:     "Quit".into(),
        icon_name: "application-exit".into(),
        activate:  Box::new(|this: &mut Self| {
          this.qt.invoke_mut(AppController::request_quit);
        }),
        ..Default::default()
      }
      .into(),
    ]
  }
}

fn runtime_dir() -> PathBuf
{
  BaseDirs::new()
    .map(|b| {
      b.runtime_dir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| b.home_dir().join(".cache"))
    })
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join("l-tasktabs")
}

fn acquire_instance_lock() -> io::Result<InstanceLock>
{
  let dir  = runtime_dir();
  let path = dir.join("instance.lock");
  std::fs::create_dir_all(&dir)?;
  let file = OpenOptions::new()
    .create(true)
    .read(true)
    .write(true)
    .open(&path)?;

  file.try_lock_exclusive().map_err(|e| {
    match (e.kind() == io::ErrorKind::WouldBlock, e.raw_os_error())
    {
      (true, _) | (_, Some(11) | Some(35)) => io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{APP_NAME} is already running (lock {})", path.display()),
      ),
      _ => e,
    }
  })?;
  Ok(InstanceLock { _file: file })
}

fn ensure_xdg_dirs()
{
  let Some(proj) = Config::project_dirs()
  else
  {
    return;
  };
  let _ = std::fs::create_dir_all(proj.config_dir());
  let _ = std::fs::create_dir_all(proj.data_dir());
  let _ = std::fs::create_dir_all(proj.state_dir().unwrap_or(proj.data_dir()));
  let _ = std::fs::create_dir_all(proj.cache_dir());
}

pub struct AppController
{
  window_visible:  bool,
  config:          Config,
  config_path:     PathBuf,
  status_message:  String,
  dirty:           bool,
}

impl Default for AppController
{
  fn default() -> Self
  {
    match Config::load_or_create()
    {
      Ok((config, path)) => Self {
        window_visible: false,
        config,
        status_message: format!("Loaded {}", path.display()),
        config_path:    path,
        dirty:          false,
      },
      Err(err) => Self {
        window_visible: false,
        config:         Config::default(),
        config_path:    Config::config_path().unwrap_or_else(|| PathBuf::from("config.toml")),
        status_message: format!("Config error: {err}"),
        dirty:          false,
      },
    }
  }
}

impl AppController
{
  fn mark_dirty(&mut self, msg: impl Into<String>)
  {
    self.dirty          = true;
    self.status_message = msg.into();
    self.config_changed();
  }

  fn sessions_json_value(&self) -> JsonValue
  {
    serde_json::to_value(&self.config.sessions).unwrap_or_else(|_| json!([]))
  }
}

#[qobject(Singleton)]
impl AppController
{
  #[qslot(qml_name = "showMainWindow")]
  fn show_main_window(&mut self)
  {
    self.window_visible = true;
    self.window_visibility_changed(true);
  }

  #[qslot(qml_name = "hideMainWindow")]
  fn hide_main_window(&mut self)
  {
    self.window_visible = false;
    self.window_visibility_changed(false);
  }

  #[qslot(qml_name = "requestQuit")]
  fn request_quit(&mut self)
  {
    self.quit_requested();
    // Tray thread parks forever after ksni spawn; Qt.quit() alone can leave the
    // process alive. Exit the whole process (window + tray).
    std::process::exit(0);
  }

  #[qslot(qml_name = "startTray")]
  fn start_tray(&self)
  {
    static START: Once = Once::new();
    let qt = self.get_thread_handle();
    START.call_once(move || {
      std::thread::Builder::new()
        .name("ltt-tray".into())
        .spawn(move || match (LttTray { qt }).spawn()
        {
          Ok(_handle) => loop
          {
            std::thread::park();
          },
          Err(err) => eprintln!("{APP_NAME}: failed to start tray: {err}"),
        })
        .expect("spawn tray thread");
    });
  }

  #[qsignal(qml_name = "windowVisibilityChanged")]
  fn window_visibility_changed(&mut self, visible: bool);

  #[qsignal(qml_name = "quitRequested")]
  fn quit_requested(&mut self);

  #[qsignal(qml_name = "configChanged")]
  fn config_changed(&mut self);

  #[qslot(qml_name = "appId")]
  fn app_id(&self) -> String
  {
    APP_ID.to_string()
  }

  #[qslot(qml_name = "appName")]
  fn app_name(&self) -> String
  {
    APP_NAME.to_string()
  }

  #[qslot(qml_name = "configPath")]
  fn config_path_slot(&self) -> String
  {
    self.config_path.display().to_string()
  }

  #[qslot(qml_name = "statusMessage")]
  fn status_message_slot(&self) -> String
  {
    self.status_message.clone()
  }

  #[qslot(qml_name = "isDirty")]
  fn is_dirty(&self) -> bool
  {
    self.dirty
  }

  #[qslot(qml_name = "hideOnClose")]
  fn hide_on_close(&self) -> bool
  {
    self.config.app.hide_on_close
  }

  #[qslot(qml_name = "setHideOnClose")]
  fn set_hide_on_close(&mut self, value: bool)
  {
    match self.config.app.hide_on_close == value
    {
      true  => {}
      false =>
      {
        self.config.app.hide_on_close = value;
        self.mark_dirty("Hide on close updated (unsaved)");
      }
    }
  }

  #[qslot(qml_name = "autostart")]
  fn autostart(&self) -> bool
  {
    self.config.app.autostart
  }

  #[qslot(qml_name = "setAutostart")]
  fn set_autostart(&mut self, value: bool)
  {
    match self.config.app.autostart == value
    {
      true  => {}
      false =>
      {
        self.config.app.autostart = value;
        self.mark_dirty("Autostart updated (unsaved; desktop entry in a later MVP)");
      }
    }
  }

  #[qslot(qml_name = "elevationMode")]
  fn elevation_mode(&self) -> String
  {
    self.config.app.elevation_mode.as_str().to_string()
  }

  #[qslot(qml_name = "setElevationMode")]
  fn set_elevation_mode(&mut self, mode: String)
  {
    match ElevationMode::parse(&mode)
    {
      Some(parsed) if parsed != self.config.app.elevation_mode =>
      {
        self.config.app.elevation_mode = parsed;
        self.mark_dirty("Elevation mode updated (unsaved)");
      }
      Some(_) => {}
      None =>
      {
        self.status_message = format!("Unknown elevation mode: {mode}");
        self.config_changed();
      }
    }
  }

  #[qslot(qml_name = "elevationModesJson")]
  fn elevation_modes_json(&self) -> JsonValue
  {
    json!(
      ElevationMode::all_labels()
        .iter()
        .map(|(id, label)| json!({ "id": id, "label": label }))
        .collect::<Vec<_>>()
    )
  }

  #[qslot(qml_name = "sessionsJson")]
  fn sessions_json(&self) -> JsonValue
  {
    self.sessions_json_value()
  }

  #[qslot(qml_name = "upsertSessionJson")]
  fn upsert_session_json(&mut self, payload: JsonValue) -> String
  {
    let session: SessionConfig = match serde_json::from_value(payload)
    {
      Ok(s)    => s,
      Err(err) => return format!("Invalid session: {err}"),
    };
    match self.config.upsert_session(session)
    {
      Ok(())   =>
      {
        self.mark_dirty("Session saved in memory (click Save to write config.toml)");
        String::new()
      }
      Err(err) => err,
    }
  }

  #[qslot(qml_name = "removeSession")]
  fn remove_session(&mut self, id: String) -> bool
  {
    match self.config.remove_session(&id)
    {
      true  =>
      {
        self.mark_dirty(format!("Removed session '{id}' (unsaved)"));
        true
      }
      false => false,
    }
  }

  #[qslot(qml_name = "saveConfig")]
  fn save_config(&mut self) -> String
  {
    match self.config.save_to(&self.config_path)
    {
      Ok(()) =>
      {
        self.dirty          = false;
        self.status_message = format!("Saved {}", self.config_path.display());
        self.config_changed();
        String::new()
      }
      Err(err) =>
      {
        self.status_message = format!("Save failed: {err}");
        self.config_changed();
        self.status_message.clone()
      }
    }
  }

  #[qslot(qml_name = "reloadConfig")]
  fn reload_config(&mut self) -> String
  {
    match Config::load_from(&self.config_path)
    {
      Ok(cfg) =>
      {
        self.config         = cfg;
        self.dirty          = false;
        self.status_message = format!("Reloaded {}", self.config_path.display());
        self.config_changed();
        String::new()
      }
      Err(err) =>
      {
        self.status_message = format!("Reload failed: {err}");
        self.config_changed();
        self.status_message.clone()
      }
    }
  }
}

fn main() -> ExitCode
{
  ensure_xdg_dirs();

  let _lock = match acquire_instance_lock()
  {
    Ok(lock) => lock,
    Err(e) if e.kind() == io::ErrorKind::AlreadyExists =>
    {
      eprintln!("{e}");
      eprintln!("Tip: use the existing tray icon (left-click shows the window).");
      return ExitCode::from(0);
    }
    Err(e) =>
    {
      eprintln!("failed to acquire instance lock: {e}");
      return ExitCode::FAILURE;
    }
  };

  QApp::new()
    .register::<AppController>()
    .load_qml(include_bytes!("../qml/Main.qml"))
    .run();

  ExitCode::SUCCESS
}
