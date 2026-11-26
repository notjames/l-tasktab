//! Builtin PTY sessions: login+interactive shell → exec, vt100 screen, survive hide.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use qtbridge::QThreadHandle;
use uuid::Uuid;

use crate::config::SessionConfig;
use crate::AppController;

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const SCROLLBACK:   usize = 5_000;
const NOTIFY_MS:    u64 = 33;

fn nz(n: u16) -> NonZeroU16
{
  NonZeroU16::new(n.max(2)).unwrap_or(NonZeroU16::MIN)
}

fn expand_cwd(cwd: &str) -> PathBuf
{
  let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
  match (cwd, home)
  {
    ("~", Some(h)) | ("", Some(h)) => h,
    (p, Some(h)) if p.starts_with("~/") => h.join(&p[2..]),
    (p, _) => PathBuf::from(p),
  }
}

fn user_shell() -> String
{
  std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
}

fn build_command(cfg: &SessionConfig) -> CommandBuilder
{
  let shell   = user_shell();
  let mut cmd = CommandBuilder::new(&shell);
  let cwd     = expand_cwd(&cfg.working_directory);
  cmd.cwd(cwd);

  for (k, v) in &cfg.env
  {
    cmd.env(k, v);
  }

  match cfg.command.trim().is_empty()
  {
    // Shell-only: login+interactive with short immutable prompt.
    true =>
    {
      cmd.env("PS1", "ltt:\\w$ ");
      cmd.arg("-li");
    }
    // Command: login+interactive, then exec (no interactive PS1 in the tab).
    false =>
    {
      cmd.arg("-lic");
      cmd.arg(format!("exec {}", cfg.command.trim()));
    }
  }
  cmd
}

struct ScreenState
{
  parser: vt100::Parser,
}

pub struct RunningSession
{
  pub runtime_id: String,
  pub config_id:  String,
  pub name:       String,
  pub elevated:   bool,
  screen:         Arc<Mutex<ScreenState>>,
  writer:         Arc<Mutex<Box<dyn Write + Send>>>,
  master:         Box<dyn MasterPty + Send>,
  child:          Box<dyn Child + Send + Sync>,
  alive:          Arc<AtomicBool>,
  rows:           u16,
  cols:           u16,
}

impl RunningSession
{
  pub fn spawn(cfg: &SessionConfig, qt: QThreadHandle<AppController>) -> Result<Self>
  {
    let runtime_id = Uuid::new_v4().to_string();
    let size = PtySize {
      rows:         DEFAULT_ROWS,
      cols:         DEFAULT_COLS,
      pixel_width:  0,
      pixel_height: 0,
    };
    let pty_system = native_pty_system();
    let pair       = pty_system.openpty(size).context("openpty")?;
    let child      = pair
      .slave
      .spawn_command(build_command(cfg))
      .context("spawn session command")?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().context("clone pty reader")?;
    let writer = pair.master.take_writer().context("take pty writer")?;
    let writer = Arc::new(Mutex::new(writer));
    let screen = Arc::new(Mutex::new(ScreenState {
      parser: vt100::Parser::new(nz(DEFAULT_ROWS), nz(DEFAULT_COLS), SCROLLBACK),
    }));
    let alive = Arc::new(AtomicBool::new(true));

    let screen_r = Arc::clone(&screen);
    let alive_r  = Arc::clone(&alive);
    let rid      = runtime_id.clone();
    thread::Builder::new()
      .name(format!("ltt-pty-{rid}"))
      .spawn(move || read_loop(reader, screen_r, alive_r, qt, rid))
      .context("spawn pty reader")?;

    Ok(Self {
      runtime_id,
      config_id: cfg.id.clone(),
      name:      cfg.name.clone(),
      elevated:  cfg.elevated,
      screen,
      writer,
      master: pair.master,
      child,
      alive,
      rows: DEFAULT_ROWS,
      cols: DEFAULT_COLS,
    })
  }

  pub fn snapshot(&self) -> String
  {
    match self.screen.lock()
    {
      Ok(g)  => g.parser.screen().contents(),
      Err(_) => String::new(),
    }
  }

  pub fn write_bytes(&self, data: &[u8]) -> Result<()>
  {
    let mut w = self.writer.lock().expect("pty writer lock");
    w.write_all(data).context("pty write")?;
    w.flush().ok();
    Ok(())
  }

  pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()>
  {
    let rows = rows.max(2);
    let cols = cols.max(2);
    match self.rows == rows && self.cols == cols
    {
      true  => return Ok(()),
      false => {}
    }
    self.master
      .resize(PtySize {
        rows,
        cols,
        pixel_width:  0,
        pixel_height: 0,
      })
      .context("pty resize")?;
    match self.screen.lock()
    {
      Ok(mut g) => g.parser.set_size(nz(rows), nz(cols)),
      Err(_)    => {}
    }
    self.rows = rows;
    self.cols = cols;
    Ok(())
  }

  pub fn stop(&mut self)
  {
    self.alive.store(false, Ordering::SeqCst);
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

fn read_loop(
  mut reader: Box<dyn Read + Send>,
  screen:     Arc<Mutex<ScreenState>>,
  alive:      Arc<AtomicBool>,
  qt:         QThreadHandle<AppController>,
  runtime_id: String,
)
{
  let mut buf      = [0u8; 8192];
  let mut last_n   = Instant::now() - Duration::from_millis(NOTIFY_MS);
  loop
  {
    match alive.load(Ordering::SeqCst)
    {
      false => break,
      true  => {}
    }
    match reader.read(&mut buf)
    {
      Ok(0) => break,
      Ok(n) =>
      {
        match screen.lock()
        {
          Ok(mut g) => g.parser.process(&buf[..n]),
          Err(_)    => break,
        }
        match last_n.elapsed() >= Duration::from_millis(NOTIFY_MS)
        {
          true =>
          {
            last_n = Instant::now();
            let id = runtime_id.clone();
            qt.invoke_mut(move |app: &mut AppController| {
              app.on_terminal_output(id);
            });
          }
          false => {}
        }
      }
      Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
      Err(_) => break,
    }
  }
  alive.store(false, Ordering::SeqCst);
  let id = runtime_id;
  qt.invoke_mut(move |app: &mut AppController| {
    app.on_terminal_exited(id);
  });
}
