# Touchctl

**Touchctl** is a Rust daemon for Linux that reads multitouch input directly from `evdev` and emits virtual events through `uinput`.  
It was built as a practical experiment in low-level input handling. A self-contained alternative to the compositor and `libinput` gesture stack.

---

## Overview
Touchctl isn’t meant to be a desktop tool. It’s a reference implementation for developers interested in the raw mechanics of Linux multitouch.  
The goal is to show how gestures, slots, synchronization events, and virtual device output all fit together without any higher-level libraries.  
It provides a working, modular example of how a compositor could implement gesture recognition and event synthesis entirely in Rust.

---

## Features
- Background daemon with clean IPC via Unix sockets  
- CLI for `start`, `stop`, `reload`, `status`, `doctor`, and `use <profile>`  
- Direct multitouch handling through `evdev`  
- Configurable gesture profiles stored in `~/.config/touchctl/profiles/`  
- `uinput` device for gesture-based mouse and keyboard events  
- Modular structure (IPC, gesture detection, tracking, action dispatch)  
- Includes udev rules and a `systemd --user` service unit  

---

## Status
Touchctl is experimental and operates at the same level as the compositor, which means it can interfere with desktop gesture systems.  
It’s most stable on lightweight window managers or clean TTY sessions.  
Scrolling, tapping, and basic pinch gestures work reliably, but timing and input arbitration are still under active refinement.

---

## Build & Run

```bash
git clone https://github.com/Jadevit/touchctl.git
cd touchctl
cargo build --release
./target/release/touchctl start
```

To stop the daemon:
```bash
touchctl stop
```

To reload gesture profiles:
```bash
touchctl reload
```

To list detected multitouch devices:
```bash
touchctl doctor
```

---

## Architecture

```text
evdev (multitouch) → Tracker → GestureDetector → Dispatcher → uinput (virtual device)
                               │
                               └── IPC (Unix socket) ⇄ touchctl CLI
```

---

## License
MIT License — see [LICENSE](./LICENSE) for details.