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

/// Which runtime should wake the UI (active terminal tab).
pub type FocusedRuntime = Arc<Mutex<Option<String>>>;

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
  html_dirty:     Arc<AtomicBool>,
  html_cache:     Arc<Mutex<Option<String>>>,
  rows:           u16,
  cols:           u16,
}

impl RunningSession
{
  pub fn spawn(
    cfg:     &SessionConfig,
    qt:      QThreadHandle<AppController>,
    focused: FocusedRuntime,
  ) -> Result<Self>
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
    let alive      = Arc::new(AtomicBool::new(true));
    let html_dirty = Arc::new(AtomicBool::new(true));
    let html_cache = Arc::new(Mutex::new(None));

    let screen_r = Arc::clone(&screen);
    let alive_r  = Arc::clone(&alive);
    let dirty_r  = Arc::clone(&html_dirty);
    let rid      = runtime_id.clone();
    thread::Builder::new()
      .name(format!("ltt-pty-{rid}"))
      .spawn(move || read_loop(reader, screen_r, alive_r, dirty_r, focused, qt, rid))
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
      html_dirty,
      html_cache,
      rows: DEFAULT_ROWS,
      cols: DEFAULT_COLS,
    })
  }

  pub fn snapshot(&self) -> String
  {
    match self.html_dirty.load(Ordering::Relaxed)
    {
      false =>
      {
        match self.html_cache.lock()
        {
          Ok(g) => match g.as_ref()
          {
            Some(html) => return html.clone(),
            None       => {}
          },
          Err(_) => {}
        }
      }
      true => {}
    }

    let html = match self.screen.lock()
    {
      Ok(g)  => screen_to_html(g.parser.screen()),
      Err(_) => String::new(),
    };
    match self.html_cache.lock()
    {
      Ok(mut g) => *g = Some(html.clone()),
      Err(_)    => {}
    }
    self.html_dirty.store(false, Ordering::Relaxed);
    html
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
    self.html_dirty.store(true, Ordering::Relaxed);
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

/// Default terminal chrome (matches QML pane).
const DEFAULT_FG: (u8, u8, u8) = (0xcd, 0xd6, 0xf4);
const DEFAULT_BG: (u8, u8, u8) = (0x1e, 0x1e, 0x2e);

fn ansi_idx_rgb(idx: u8) -> (u8, u8, u8)
{
  // xterm 256-color table
  static BASIC: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00), (0xcd, 0x00, 0x00), (0x00, 0xcd, 0x00), (0xcd, 0xcd, 0x00),
    (0x00, 0x00, 0xee), (0xcd, 0x00, 0xcd), (0x00, 0xcd, 0xcd), (0xe5, 0xe5, 0xe5),
    (0x7f, 0x7f, 0x7f), (0xff, 0x00, 0x00), (0x00, 0xff, 0x00), (0xff, 0xff, 0x00),
    (0x5c, 0x5c, 0xff), (0xff, 0x00, 0xff), (0x00, 0xff, 0xff), (0xff, 0xff, 0xff),
  ];
  match idx
  {
    0..=15 => BASIC[idx as usize],
    16..=231 =>
    {
      let n = idx - 16;
      let r = n / 36;
      let g = (n % 36) / 6;
      let b = n % 6;
      let ramp = |v: u8| -> u8 {
        match v
        {
          0 => 0,
          _ => 55 + 40 * v,
        }
      };
      (ramp(r), ramp(g), ramp(b))
    }
    232..=255 =>
    {
      let v = 8 + 10 * (idx - 232);
      (v, v, v)
    }
  }
}

fn color_rgb(c: vt100::Color, default: (u8, u8, u8)) -> (u8, u8, u8)
{
  match c
  {
    vt100::Color::Default   => default,
    vt100::Color::Idx(i)    => ansi_idx_rgb(i),
    vt100::Color::Rgb(r, g, b) => (r, g, b),
  }
}

fn css_rgb((r, g, b): (u8, u8, u8)) -> String
{
  format!("#{r:02x}{g:02x}{b:02x}")
}

fn html_escape_push(out: &mut String, s: &str)
{
  for ch in s.chars()
  {
    match ch
    {
      '&'  => out.push_str("&amp;"),
      '<'  => out.push_str("&lt;"),
      '>'  => out.push_str("&gt;"),
      '"'  => out.push_str("&quot;"),
      ' '  => out.push_str("&nbsp;"),
      '\n' => out.push_str("<br/>"),
      _    => out.push(ch),
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RunStyle
{
  fg:        (u8, u8, u8),
  bg:        (u8, u8, u8),
  bold:      bool,
  italic:    bool,
  underline: bool,
}

fn cell_style(cell: &vt100::Cell) -> RunStyle
{
  let mut fg = color_rgb(cell.fgcolor(), DEFAULT_FG);
  let mut bg = color_rgb(cell.bgcolor(), DEFAULT_BG);
  match cell.inverse()
  {
    true  =>
    {
      let t = fg;
      fg = bg;
      bg = t;
    }
    false => {}
  }
  RunStyle {
    fg,
    bg,
    bold:      cell.bold(),
    italic:    cell.italic(),
    underline: cell.underline(),
  }
}

fn push_styled_run(out: &mut String, style: RunStyle, text: &str)
{
  match text.is_empty()
  {
    true  => return,
    false => {}
  }
  out.push_str("<span style=\"color:");
  out.push_str(&css_rgb(style.fg));
  out.push_str(";background-color:");
  out.push_str(&css_rgb(style.bg));
  match style.bold
  {
    true  => out.push_str(";font-weight:bold"),
    false => {}
  }
  match style.italic
  {
    true  => out.push_str(";font-style:italic"),
    false => {}
  }
  match style.underline
  {
    true  => out.push_str(";text-decoration:underline"),
    false => {}
  }
  out.push_str("\">");
  html_escape_push(out, text);
  out.push_str("</span>");
}

/// Render vt100 screen as Qt RichText HTML with ANSI colors.
fn screen_to_html(screen: &vt100::Screen) -> String
{
  let (rows_nz, cols_nz) = screen.size();
  let rows = rows_nz.get();
  let cols = cols_nz.get();
  let mut out = String::with_capacity((rows as usize) * (cols as usize) * 8);
  out.push_str(
    "<body style=\"margin:0;white-space:pre;font-family:monospace;background-color:#1e1e2e;color:#cdd6f4;\">",
  );

  for row in 0..rows
  {
    let mut run_style: Option<RunStyle> = None;
    let mut run_text  = String::new();

    for col in 0..cols
    {
      let Some(cell) = screen.cell(row, col)
      else
      {
        continue;
      };
      match cell.is_wide_continuation()
      {
        true  => continue,
        false => {}
      }

      let style = cell_style(cell);
      let piece = match cell.has_contents()
      {
        true  => cell.contents(),
        false => " ",
      };

      match run_style
      {
        Some(s) if s == style => run_text.push_str(piece),
        Some(s) =>
        {
          push_styled_run(&mut out, s, &run_text);
          run_style = Some(style);
          run_text.clear();
          run_text.push_str(piece);
        }
        None =>
        {
          run_style = Some(style);
          run_text.push_str(piece);
        }
      }
    }

    match run_style
    {
      Some(s) => push_styled_run(&mut out, s, &run_text),
      None    => {}
    }
    out.push_str("<br/>");
  }

  out.push_str("</body>");
  out
}

fn read_loop(
  mut reader: Box<dyn Read + Send>,
  screen:     Arc<Mutex<ScreenState>>,
  alive:      Arc<AtomicBool>,
  html_dirty: Arc<AtomicBool>,
  focused:    FocusedRuntime,
  qt:         QThreadHandle<AppController>,
  runtime_id: String,
)
{
  let mut buf    = [0u8; 8192];
  let mut last_n = Instant::now() - Duration::from_millis(NOTIFY_MS);
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
        html_dirty.store(true, Ordering::Relaxed);

        // Only wake QML for the focused tab — inactive PTYs keep running silently.
        let is_focused = match focused.lock()
        {
          Ok(g)  => g.as_ref() == Some(&runtime_id),
          Err(_) => false,
        };
        match is_focused && last_n.elapsed() >= Duration::from_millis(NOTIFY_MS)
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
