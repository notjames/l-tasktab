//! L-TaskTabs MVP 1 — QML main window, StatusNotifier tray, single-instance lock.
//!
//! Qt Bridge uses `QGuiApplication`, so `Qt.labs.platform.SystemTrayIcon` (QWidget)
//! cannot be used. Tray is implemented with `ksni` (freedesktop StatusNotifierItem).

use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Once;

use directories::{BaseDirs, ProjectDirs};
use fs2::FileExt;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use qtbridge::{QApp, QThreadHandle, QmlObject, qobject};

const APP_ID: &str = "com.jimconn.ltasktabs";
const APP_NAME: &str = "L-TaskTabs";

/// Holds the exclusive instance lock for the process lifetime.
struct InstanceLock {
    _file: std::fs::File,
}

struct LttTray {
    qt: QThreadHandle<AppController>,
}

impl ksni::Tray for LttTray {
    fn id(&self) -> String {
        APP_ID.into()
    }

    fn title(&self) -> String {
        APP_NAME.into()
    }

    fn icon_name(&self) -> String {
        "utilities-terminal".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    /// Left-click → show main window.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.qt.invoke_mut(AppController::show_main_window);
    }

    /// Right-click menu.
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Show L-TaskTabs".into(),
                icon_name: "document-open".into(),
                activate: Box::new(|this: &mut Self| {
                    this.qt.invoke_mut(AppController::show_main_window);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|this: &mut Self| {
                    this.qt.invoke_mut(AppController::request_quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn runtime_dir() -> PathBuf {
    BaseDirs::new()
        .map(|b| {
            b.runtime_dir()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| b.home_dir().join(".cache"))
        })
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("l-tasktabs")
}

fn acquire_instance_lock() -> io::Result<InstanceLock> {
    let dir = runtime_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("instance.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)?;
    file.try_lock_exclusive().map_err(|e| {
        if e.kind() == io::ErrorKind::WouldBlock
            || e.raw_os_error() == Some(11)
            || e.raw_os_error() == Some(35)
        {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{APP_NAME} is already running (lock {})",
                    path.display()
                ),
            )
        } else {
            e
        }
    })?;
    Ok(InstanceLock { _file: file })
}

fn ensure_xdg_dirs() {
    if let Some(proj) = ProjectDirs::from("com", "jimconn", "l-tasktabs") {
        let _ = std::fs::create_dir_all(proj.config_dir());
        let _ = std::fs::create_dir_all(proj.data_dir());
        let _ = std::fs::create_dir_all(proj.state_dir().unwrap_or(proj.data_dir()));
        let _ = std::fs::create_dir_all(proj.cache_dir());
    }
}

#[derive(Default)]
pub struct AppController {
    window_visible: bool,
}

#[qobject(Singleton)]
impl AppController {
    #[qslot(qml_name = "showMainWindow")]
    fn show_main_window(&mut self) {
        self.window_visible = true;
        self.window_visibility_changed(true);
    }

    #[qslot(qml_name = "hideMainWindow")]
    fn hide_main_window(&mut self) {
        self.window_visible = false;
        self.window_visibility_changed(false);
    }

    #[qslot(qml_name = "requestQuit")]
    fn request_quit(&mut self) {
        self.quit_requested();
    }

    #[qslot(qml_name = "startTray")]
    fn start_tray(&self) {
        static START: Once = Once::new();
        let qt = self.get_thread_handle();
        START.call_once(move || {
            std::thread::Builder::new()
                .name("ltt-tray".into())
                .spawn(move || match (LttTray { qt }).spawn() {
                    Ok(_handle) => loop {
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

    #[qslot(qml_name = "appId")]
    fn app_id(&self) -> String {
        APP_ID.to_string()
    }

    #[qslot(qml_name = "appName")]
    fn app_name(&self) -> String {
        APP_NAME.to_string()
    }
}

fn main() -> ExitCode {
    ensure_xdg_dirs();

    let _lock = match acquire_instance_lock() {
        Ok(lock) => lock,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            eprintln!("{e}");
            eprintln!("Tip: use the existing tray icon (left-click shows the window).");
            return ExitCode::from(0);
        }
        Err(e) => {
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
