//! L-TaskTabs — tray app with XDG config and builtin PTY terminal tabs.

mod config;
mod pty;

use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Once};

use directories::BaseDirs;
use fs2::FileExt;
use image::GenericImageView;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use qtbridge::{QApp, QThreadHandle, QmlObject, qobject};
use serde_json::{json, Value as JsonValue};

use crate::config::{Config, ElevationMode, SessionConfig};
use crate::pty::{FocusedRuntime, RunningSession};

const APP_ID:   &str = "com.jimconn.ltasktabs";
const APP_NAME: &str = "L-TaskTabs";

struct InstanceLock
{
  _file: std::fs::File,
}

#[derive(Clone, Default)]
struct TrayShared
{
  window_visible:       Arc<AtomicBool>,
  quick_manage_enabled: Arc<AtomicBool>,
  /// (runtime_id, name)
  sessions:       Arc<Mutex<Vec<(String, String)>>>,
}

struct LttTray
{
  qt:     QThreadHandle<AppController>,
  shared: TrayShared,
}

static TRAY_HANDLE: Mutex<Option<ksni::blocking::Handle<LttTray>>> = Mutex::new(None);

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
    // Toggle: visible → hide; hidden → show.
    self.qt.invoke_mut(AppController::toggle_main_window);
  }

  fn secondary_activate(&mut self, x: i32, y: i32)
  {
    // Middle-click / secondary → Quick Manage popup near cursor.
    if self.shared.quick_manage_enabled.load(Ordering::Relaxed)
    {
      self.qt.invoke_mut(move |app: &mut AppController| {
        app.open_quick_manage(x, y);
      });
    }
  }

  fn tool_tip(&self) -> ksni::ToolTip
  {
    let sessions = self
      .shared
      .sessions
      .lock()
      .map(|g| g.clone())
      .unwrap_or_default();
    let description = match sessions.is_empty()
    {
      true  => "No running terminals\nMiddle-click: Quick Manage".into(),
      false =>
      {
        let mut lines: Vec<String> = sessions
          .iter()
          .map(|(_, name)| format!("• {name}"))
          .collect();
        lines.push(String::new());
        lines.push("Middle-click: Quick Manage".into());
        lines.join("\n")
      }
    };
    ksni::ToolTip {
      title:       APP_NAME.into(),
      description,
      ..Default::default()
    }
  }

  fn menu(&self) -> Vec<ksni::MenuItem<Self>>
  {
    let sessions = self
      .shared
      .sessions
      .lock()
      .map(|g| g.clone())
      .unwrap_or_default();

    let mut items = vec![
      StandardItem {
        label:     "Show window".into(),
        icon_name: "document-open".into(),
        activate:  Box::new(|this: &mut Self| {
          this.qt.invoke_mut(AppController::show_main_window);
        }),
        ..Default::default()
      }
      .into(),
      StandardItem {
        label:     "Quick Manage…".into(),
        icon_name: "preferences-system".into(),
        activate:  Box::new(|this: &mut Self| {
          this.qt.invoke_mut(|app: &mut AppController| {
            app.open_quick_manage(-1, -1);
          });
        }),
        ..Default::default()
      }
      .into(),
      ksni::MenuItem::Separator,
    ];

    for (rid, name) in sessions
    {
      let rid_show = rid.clone();
      let rid_stop = rid;
      items.push(
        StandardItem {
          label:    format!("Focus: {name}"),
          activate: Box::new(move |this: &mut Self| {
            let id = rid_show.clone();
            this.qt.invoke_mut(move |app: &mut AppController| {
              app.focus_running_session(id);
            });
          }),
          ..Default::default()
        }
        .into(),
      );
      items.push(
        StandardItem {
          label:    format!("Stop: {name}"),
          activate: Box::new(move |this: &mut Self| {
            let id = rid_stop.clone();
            this.qt.invoke_mut(move |app: &mut AppController| {
              app.stop_running_session(id);
            });
          }),
          ..Default::default()
        }
        .into(),
      );
    }

    items.push(ksni::MenuItem::Separator);
    items.push(
      StandardItem {
        label:     "Quit".into(),
        icon_name: "application-exit".into(),
        activate:  Box::new(|this: &mut Self| {
          this.qt.invoke_mut(AppController::request_quit);
        }),
        ..Default::default()
      }
      .into(),
    );
    items
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

fn check_display_environment() -> Result<(), String>
{
  // Explicit headless platforms are useful for diagnostics and should be
  // left to Qt to validate. Normal tray operation requires a desktop display.
  let platform = std::env::var("QT_QPA_PLATFORM").unwrap_or_default();
  if matches!(platform.as_str(), "offscreen" | "minimal" | "minimalegl")
  {
    return Ok(());
  }

  let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
    .iter()
    .any(|name| std::env::var(name).map(|v| !v.trim().is_empty()).unwrap_or(false));
  if has_display
  {
    return Ok(());
  }

  Err(
    "no graphical display detected (DISPLAY and WAYLAND_DISPLAY are empty).\n\
     Start l-tasktabs from a terminal inside your KDE/GNOME session, or restore \
     the session environment before retrying.\n\
     Diagnostic: echo \"DISPLAY=$DISPLAY\" \"WAYLAND_DISPLAY=$WAYLAND_DISPLAY\""
      .into(),
  )
}

fn autostart_desktop_path() -> io::Result<PathBuf>
{
  let Some(base) = BaseDirs::new()
  else
  {
    return Err(io::Error::new(
      io::ErrorKind::NotFound,
      "could not resolve the user config directory",
    ));
  };
  Ok(base.config_dir().join("autostart/l-tasktabs.desktop"))
}

fn sync_autostart_entry(enabled: bool) -> io::Result<()>
{
  let path = autostart_desktop_path()?;
  if !enabled
  {
    return match fs::remove_file(path)
    {
      Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
      result => result,
    };
  }

  let executable = std::env::current_exe()?;
  let executable = executable
    .display()
    .to_string()
    .replace('\\', "\\\\")
    .replace('"', "\\\"");
  if let Some(parent) = path.parent()
  {
    fs::create_dir_all(parent)?;
  }
  fs::write(
    path,
    format!(
      "[Desktop Entry]\n\
       Type=Application\n\
       Name=L-TaskTabs\n\
       Comment=Tray-resident TUI session manager\n\
       Exec=\"{executable}\"\n\
       Terminal=false\n\
       X-GNOME-Autostart-enabled=true\n"
    ),
  )
}

pub struct AppController
{
  window_visible:  bool,
  config:          Config,
  config_path:     PathBuf,
  status_message:  String,
  dirty:           bool,
  running:         Vec<RunningSession>,
  focused:         FocusedRuntime,
  tray_shared:     TrayShared,
}

impl Default for AppController
{
  fn default() -> Self
  {
    let tray_shared = TrayShared {
      quick_manage_enabled: Arc::new(AtomicBool::new(true)),
      ..Default::default()
    };
    match Config::load_or_create()
    {
      Ok((config, path)) => Self {
        window_visible: false,
        config,
        status_message: format!("Loaded {}", path.display()),
        config_path:    path,
        dirty:          false,
        running:        Vec::new(),
        focused:        Arc::new(Mutex::new(None)),
        tray_shared,
      },
      Err(err) => Self {
        window_visible: false,
        config:         Config::default(),
        config_path:    Config::config_path().unwrap_or_else(|| PathBuf::from("config.toml")),
        status_message: format!("Config error: {err}"),
        dirty:          false,
        running:        Vec::new(),
        focused:        Arc::new(Mutex::new(None)),
        tray_shared,
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

  fn running_json_value(&self) -> JsonValue
  {
    json!(
      self
        .running
        .iter()
        .map(|s| {
          json!({
            "runtimeId": s.runtime_id,
            "configId":  s.config_id,
            "name":      s.name,
            "elevated":  s.elevated,
          })
        })
        .collect::<Vec<_>>()
    )
  }

  fn sync_tray_shared(&self)
  {
    self
      .tray_shared
      .window_visible
      .store(self.window_visible, Ordering::Relaxed);
    self
      .tray_shared
      .quick_manage_enabled
      .store(self.config.app.quick_manage_enabled, Ordering::Relaxed);
    match self.tray_shared.sessions.lock()
    {
      Ok(mut g) =>
      {
        *g = self
          .running
          .iter()
          .map(|s| (s.runtime_id.clone(), s.name.clone()))
          .collect();
      }
      Err(_) => {}
    }
    match TRAY_HANDLE.lock()
    {
      Ok(g) =>
      {
        match g.as_ref()
        {
          Some(h) =>
          {
            let _ = h.update(|_| {});
          }
          None => {}
        }
      }
      Err(_) => {}
    }
  }

  pub fn on_terminal_html(&mut self, runtime_id: String, html: String)
  {
    // Always emit — QML keeps a per-runtime cache so frames are never lost to races.
    self.terminal_updated(runtime_id, html);
  }

  pub fn on_terminal_exited(&mut self, runtime_id: String)
  {
    let name = self
      .running
      .iter()
      .find(|s| s.runtime_id == runtime_id)
      .map(|s| s.name.clone())
      .unwrap_or_else(|| runtime_id.clone());
    self.running.retain(|s| s.runtime_id != runtime_id);
    match self.focused.lock()
    {
      Ok(mut g) if g.as_ref() == Some(&runtime_id) => *g = self.running.first().map(|s| s.runtime_id.clone()),
      _ => {}
    }
    self.status_message = format!(
      "Session '{name}' ended. Check that its command exists and is on PATH."
    );
    self.sync_tray_shared();
    self.running_changed();
    // Keep the last frame available long enough for the status message to
    // explain why a short-lived command disappeared.
    self.config_changed();
  }

  fn stop_all_running(&mut self)
  {
    for s in &mut self.running
    {
      s.stop();
    }
    self.running.clear();
    match self.focused.lock()
    {
      Ok(mut g) => *g = None,
      Err(_)    => {}
    }
    self.sync_tray_shared();
  }

  pub fn open_quick_manage(&mut self, x: i32, y: i32)
  {
    self.quick_manage_requested(x, y);
  }

  pub fn focus_running_session(&mut self, runtime_id: String)
  {
    let html = self
      .running
      .iter()
      .find(|s| s.runtime_id == runtime_id)
      .map(|s| s.snapshot())
      .unwrap_or_default();
    match self.focused.lock()
    {
      Ok(mut g) => *g = Some(runtime_id.clone()),
      Err(_)    => {}
    }
    self.show_main_window();
    self.running_changed();
    self.terminal_updated(runtime_id, html);
  }
}

#[qobject(Singleton)]
impl AppController
{
  #[qslot(qml_name = "showMainWindow")]
  fn show_main_window(&mut self)
  {
    self.window_visible = true;
    self.sync_tray_shared();
    self.window_visibility_changed(true);
  }

  #[qslot(qml_name = "hideMainWindow")]
  fn hide_main_window(&mut self)
  {
    self.window_visible = false;
    self.sync_tray_shared();
    self.window_visibility_changed(false);
  }

  #[qslot(qml_name = "toggleMainWindow")]
  fn toggle_main_window(&mut self)
  {
    match self.window_visible
    {
      true  => self.hide_main_window(),
      false => self.show_main_window(),
    }
  }

  #[qslot(qml_name = "requestQuit")]
  fn request_quit(&mut self)
  {
    self.stop_all_running();
    let tray = TRAY_HANDLE
      .lock()
      .ok()
      .and_then(|g| g.as_ref().cloned());
    if let Some(handle) = tray
    {
      // Sending shutdown is enough here. Waiting synchronously would block
      // the Qt thread that must process Qt.quit().
      let _ = handle.shutdown();
    }
    self.quit_requested();
    // The Qt event loop does not reliably terminate when the request
    // originates from the detached StatusNotifier thread. Force process
    // termination after the application-owned sessions and tray are stopped.
    std::process::exit(0);
  }

  #[qslot(qml_name = "startTray")]
  fn start_tray(&self)
  {
    static START: Once = Once::new();
    let qt     = self.get_thread_handle();
    let shared = self.tray_shared.clone();
    START.call_once(move || {
      std::thread::Builder::new()
        .name("ltt-tray".into())
        .spawn(move || match (LttTray { qt, shared }).spawn()
        {
          Ok(handle) =>
          {
            match TRAY_HANDLE.lock()
            {
              Ok(mut g) => *g = Some(handle),
              Err(_)    => {}
            }
            loop
            {
              std::thread::park();
            }
          }
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

  #[qsignal(qml_name = "runningChanged")]
  fn running_changed(&mut self);

  #[qsignal(qml_name = "terminalUpdated")]
  fn terminal_updated(&mut self, runtime_id: String, html: String);

  #[qsignal(qml_name = "sessionStarted")]
  fn session_started(&mut self, runtime_id: String);

  #[qsignal(qml_name = "quickManageRequested")]
  fn quick_manage_requested(&mut self, x: i32, y: i32);

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

  #[qslot(qml_name = "quickManageEnabled")]
  fn quick_manage_enabled(&self) -> bool
  {
    self.config.app.quick_manage_enabled
  }

  #[qslot(qml_name = "setQuickManageEnabled")]
  fn set_quick_manage_enabled(&mut self, value: bool)
  {
    if self.config.app.quick_manage_enabled != value
    {
      self.config.app.quick_manage_enabled = value;
      self.mark_dirty("Tray Quick Manage binding updated (unsaved)");
      self.sync_tray_shared();
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
      Err(err) => return format!("ERR:Invalid session: {err}"),
    };
    let name = session.name.clone();
    match self.config.upsert_session(session)
    {
      Ok(()) =>
      {
        let id = self
          .config
          .sessions
          .iter()
          .rev()
          .find(|s| s.name == name)
          .map(|s| s.id.clone())
          .unwrap_or_default();
        self.mark_dirty("Session saved in memory (click Save to write config.toml)");
        id
      }
      Err(err) => format!("ERR:{err}"),
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
        if let Err(err) = sync_autostart_entry(self.config.app.autostart)
        {
          self.status_message = format!(
            "Config saved, but autostart update failed: {err}"
          );
        }
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

  #[qslot(qml_name = "runningSessionsJson")]
  fn running_sessions_json(&self) -> JsonValue
  {
    self.running_json_value()
  }

  #[qslot(qml_name = "startConfiguredSession")]
  fn start_configured_session(&mut self, config_id: String) -> String
  {
    let Some(cfg) = self.config.sessions.iter().find(|s| s.id == config_id).cloned()
    else
    {
      return format!("Unknown session id");
    };
    if cfg.elevated && self.config.app.elevation_mode == ElevationMode::Keyring
    {
      self.status_message = format!(
        "Session '{}' was not started: keyring elevation is not implemented yet.",
        cfg.name
      );
      self.config_changed();
      return self.status_message.clone();
    }
    let qt      = self.get_thread_handle();
    let focused = Arc::clone(&self.focused);
    let elevation_mode = self.config.app.elevation_mode.clone();
    match RunningSession::spawn(&cfg, elevation_mode, qt, focused)
    {
      Ok(session) =>
      {
        let rid  = session.runtime_id.clone();
        let name = session.name.clone();
        // Focus the session we just started. QML mirrors this via sessionStarted
        // (selects the new tab) so Rust push + visible pane stay aligned.
        match self.focused.lock()
        {
          Ok(mut g) => *g = Some(rid.clone()),
          Err(_)    => {}
        }
        self.status_message = format!("Started {name}");
        self.running.push(session);
        self.sync_tray_shared();
        self.running_changed();
        self.session_started(rid.clone());
        rid
      }
      Err(err) =>
      {
        self.status_message = format!("Start failed: {err:#}");
        self.config_changed();
        format!("Start failed: {err:#}")
      }
    }
  }

  #[qslot(qml_name = "stopRunningSession")]
  fn stop_running_session(&mut self, runtime_id: String) -> bool
  {
    let mut found = false;
    self.running.retain_mut(|s| {
      match s.runtime_id == runtime_id
      {
        true =>
        {
          s.stop();
          found = true;
          false
        }
        false => true,
      }
    });
    match found
    {
      true =>
      {
        match self.focused.lock()
        {
          Ok(mut g) if g.as_ref() == Some(&runtime_id) =>
          {
            *g = self.running.first().map(|s| s.runtime_id.clone());
          }
          _ => {}
        }
        self.status_message = "Stopped session".into();
        self.sync_tray_shared();
        self.running_changed();
        true
      }
      false => false,
    }
  }

  #[qslot(qml_name = "setFocusedRuntime")]
  fn set_focused_runtime(&mut self, runtime_id: String)
  {
    match self.focused.lock()
    {
      Ok(mut g) => *g = match runtime_id.is_empty()
      {
        true  => None,
        false => Some(runtime_id.clone()),
      },
      Err(_) => {}
    }
    // Push a fresh frame for the newly focused tab. Never emit an empty string
    // here — that used to wipe a good frame when the PTY had not produced yet.
    match runtime_id.is_empty()
    {
      false =>
      {
        let html = self
          .running
          .iter()
          .find(|s| s.runtime_id == runtime_id)
          .map(|s| s.snapshot())
          .unwrap_or_default();
        match html.is_empty()
        {
          false => self.terminal_updated(runtime_id, html),
          true  => {}
        }
      }
      true => {}
    }
  }

  #[qslot(qml_name = "resizeAllTerminals")]
  fn resize_all_terminals(&mut self, rows: i32, cols: i32) -> bool
  {
    let rows = rows.max(2) as u16;
    let cols = cols.max(2) as u16;
    let mut ok = true;
    for s in &mut self.running
    {
      ok &= s.resize(rows, cols).is_ok();
    }
    ok
  }

  #[qslot(qml_name = "writeTerminal")]
  fn write_terminal(&mut self, runtime_id: String, data: String) -> bool
  {
    match self.running.iter().find(|s| s.runtime_id == runtime_id)
    {
      Some(s) => s.write_bytes(data.as_bytes()).is_ok(),
      None    => false,
    }
  }

  #[qslot(qml_name = "resizeTerminal")]
  fn resize_terminal(&mut self, runtime_id: String, rows: i32, cols: i32) -> bool
  {
    let rows = rows.max(2) as u16;
    let cols = cols.max(2) as u16;
    match self.running.iter_mut().find(|s| s.runtime_id == runtime_id)
    {
      Some(s) => s.resize(rows, cols).is_ok(),
      None    => false,
    }
  }

  #[qslot(qml_name = "sendTerminalMouse")]
  fn send_terminal_mouse(
    &mut self,
    runtime_id: String,
    kind:      i32,
    button:    i32,
    col:       i32,
    row:       i32,
    modifiers: i32,
  ) -> bool
  {
    match self.running.iter().find(|s| s.runtime_id == runtime_id)
    {
      Some(s) => s.send_mouse(kind, button, col, row, modifiers).unwrap_or(false),
      None    => false,
    }
  }

  #[qslot(qml_name = "terminalSnapshot")]
  fn terminal_snapshot(&self, runtime_id: String) -> String
  {
    match self.running.iter().find(|s| s.runtime_id == runtime_id)
    {
      Some(s) => s.snapshot(),
      None    => String::new(),
    }
  }

  #[qslot(qml_name = "startAutostartSessions")]
  fn start_autostart_sessions(&mut self)
  {
    let ids: Vec<String> = self
      .config
      .sessions
      .iter()
      .filter(|s| s.autostart && !s.elevated)
      .map(|s| s.id.clone())
      .collect();
    let skipped_elevated = self
      .config
      .sessions
      .iter()
      .filter(|s| s.autostart && s.elevated)
      .map(|s| s.name.clone())
      .collect::<Vec<_>>();
    for id in ids
    {
      let _ = self.start_configured_session(id);
    }
    if !skipped_elevated.is_empty()
    {
      self.status_message = format!(
        "Autostart skipped elevated session(s): {} (start manually to approve sudo)",
        skipped_elevated.join(", ")
      );
      self.config_changed();
    }
  }
}

fn main() -> ExitCode
{
  ensure_xdg_dirs();

  if let Err(err) = check_display_environment()
  {
    eprintln!("{APP_NAME}: {err}");
    return ExitCode::FAILURE;
  }

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
