# L-TaskTabs — product plan

Living plan. **Locked** decisions are marked as such; open items stay TBD.

## Branding

| Use | Name |
|-----|------|
| Display | **L-TaskTabs** |
| Binary / repo | `l-tasktabs` |
| App id | `com.jimconn.ltasktabs` |
| Dev path | `/home/jimconn/projects/src/personal/l-tasktabs` |

### XDG paths (locked)

| Kind | Path |
|------|------|
| Config | `$XDG_CONFIG_HOME/l-tasktabs/config.toml` |
| Data | `$XDG_DATA_HOME/l-tasktabs/` |
| State | `$XDG_STATE_HOME/l-tasktabs/` |
| Cache | `$XDG_CACHE_HOME/l-tasktabs/` |
| Desktop entry | shipped / `$XDG_DATA_HOME/applications/` |
| Autostart | `$XDG_CONFIG_HOME/autostart/l-tasktabs.desktop` when enabled |

---

## Goals

- Tray-first; professional; follow DE theme where practical.
- Many configured TUI “terms” runnable **simultaneously**.
- Tabbed session UI; tabs **flush adjacent** (≈0–1px).
- Sessions **survive window hide**; end on tab close, stop, or tray Quit.
- Secure desktop defaults; elevated tabs visually distinct.
- Linux first; **x86_64** + **aarch64**; KDE + GNOME (DE-agnostic where possible).

Non-goal: Plasma plasmoid / Konsole KPart-in-plasmashell.

---

## UX (locked)

| Gesture | Behavior |
|---------|----------|
| **Left-click tray** | Open **main application window** (industry standard) |
| **Right-click tray** | **Context menu** (Show window, session shortcuts, Quit, …) |
| **Quit** | Clean shutdown of all sessions |
| **Hide/close main window** | Back to tray; sessions keep running |
| **Autostart** | Off by default; config toggle |

Main window holds configuration + session management (layout to be felt out in implementation). Tab chrome: icon + name + state dot; elevated = shield / warning color.

---

## Stack (locked)

| Layer | Choice |
|-------|--------|
| UI | **QML (Qt Quick)** |
| Rust ↔ QML | **Qt Bridge for Rust** (`qtbridge`) — primary |
| Escape hatch | **CXX-Qt co-exists** when Widgets / C++-only Qt APIs / embed needs appear |
| Session / PTY | Rust |
| Terminal | **Builtin** (in-process PTY + renderer); optional **external** emulator = separate window only |
| Qt version | **≥ 6.10** (prefer 6.12 per Qt Bridge guidance) |

### Terminal env & prompt (locked)

- **Command sessions:** start via **login + interactive** shell for user `PATH`/env, then **`exec` the command** (no interactive PS1 in the tab).
- **Shell-only sessions:** **immutable short prompt** by default (e.g. `ltt:\w$ `); optional “use user PS1” later.
- Per-session `env` map in config for overrides.
- **Elevated:** tighter/scrubbed env; visual distinction; no autostart unless explicitly opted in.

### Elevation modes (configurable)

`ask_every_time` | `sudo_ticket` | `ask_once_memory` | `keyring` (optional) — no root password in TOML.

---

## Architecture

```text
tray (single instance, com.jimconn.ltasktabs)
  left-click → main QML window
  right-click → context menu
       │
  Rust backend (qtbridge QObjects)
       │
  N builtin PTY sessions (tabs)
  optional: external emulator children
       │
  $XDG_CONFIG_HOME/l-tasktabs/config.toml
  optional: libsecret / polkit / sudo broker
```

---

## Config sketch (TOML)

```toml
[app]
autostart = false
elevation_mode = "ask_once_memory"
hide_on_close = true

[[sessions]]
id = "550e8400-e29b-41d4-a716-446655440000"  # auto UUID; not user-facing
name = "syswatch"
command = "syswatch"
working_directory = "~"
autostart = false
elevated = false
# backend = "builtin" | "external"
# emulator = "kitty"   # external only
# [sessions.env]
# FOO = "bar"
```

---

## MVP milestones

1. Skeleton: `qtbridge` + QML main window + single-instance + tray (left/right gestures) — **done** (ksni StatusNotifierItem; Qt Bridge is QGuiApplication-only so QWidget/SystemTrayIcon is not used)
2. XDG config load/save + Configure UI — **done**
3. Builtin terminal tab + PTY + one session (login+interactive → exec) — **done**
4. Multi-tab + flush tab bar + survive hide — **done**
5. Tray context menu quick actions — **partial** (tooltip list, Quick Manage popup, toggle left-click; true hover-interactive widget still TBD)
6. Elevation modes + elevated tab chrome
7. Autostart desktop entry + x86_64/aarch64 packaging notes
8. Optional external-emulator backend

---

## Still light / feel-out

- Exact main-window IA (config vs sessions layout)
- Anchored popup later vs floating window only for v1
- License for original L-TaskTabs code
- Confirm qtbridge on **aarch64 Linux** early in MVP 1

---

## Reference

Behavior inspiration: `/home/jimconn/projects/src/personal/termibar` (do not copy AGPL sources blindly).  
Qt Bridge: https://doc.qt.io/qtbridge-rust/qtbridge/index.html  
CXX-Qt (co-exist): when Bridge is insufficient.
