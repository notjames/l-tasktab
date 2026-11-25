# L-TaskTabs

Tray-resident session manager for long-lived TUI tools — one window, many tabs, processes keep running when you hide the window.

Inspired by [Termibar](https://github.com/ekats/termibar) (KDE plasmoid), implemented as a **standalone, DE-agnostic application** (KDE + GNOME first).

> Status: **MVP 1 done** — Qt Bridge + QML shell, ksni tray, single-instance. See [PLAN.md](./PLAN.md).

## Idea

Pin tools like `syswatch`, `diskwatch`, `cliAMP`, etc. so they stay available without cluttering your main terminal (Ghostty) workflow. Open one L-TaskTabs window, switch tabs, close the window — sessions keep running until you stop them or quit from the tray.

## Build (MVP 1)

Requires **Rust ≥ 1.88**, **Qt ≥ 6.10** (with private headers), and `qmake` on `PATH`.

```bash
# Debian/Ubuntu/Neon
./scripts/install-deps-debian.sh

# Ensure qmake resolves (qtbridge looks for it)
export PATH="/usr/lib/qt6/bin:$PATH"   # if needed
hash -r
qmake -query QT_VERSION                # or qmake6

cargo run -p l_tasktabs
# binary: target/debug/l-tasktabs
```

**Tray:** left-click shows the main window; right-click opens the menu (Show / Quit). Closing the window hides to tray.

**Single-instance:** lock file under `$XDG_RUNTIME_DIR/l-tasktabs/instance.lock`.

## Quick links

- Product & architecture: [PLAN.md](./PLAN.md)
- Config: `$XDG_CONFIG_HOME/l-tasktabs/`
- Binary: `l-tasktabs`
