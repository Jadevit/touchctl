# Touchctl

**Touchctl** is an experimental Rust daemon for Linux that reads multitouch input directly from `evdev` and emits virtual input events through `uinput`.  
It was built as a low-level exploration of Linux input handling without relying on `libinput`.

---

## Features
- Runs as a background daemon with CLI commands for `start`, `stop`, `reload`, `status`, and `doctor`
- Creates a virtual `uinput` device for gesture-based mouse and keyboard actions
- Loads configurable gesture profiles from `~/.config/touchctl/profiles/`
- Includes a default gesture profile for basic functionality out of the box
- Packaging assets for a user-level `systemd` service and udev rules

---

## Status
Touchctl is **experimental** and currently interacts directly with the compositor through `evdev` grabs.  
It works, but it’s rough around the edges — expect input conflicts and unpredictable behavior on GNOME and other compositors.

---

## Build and Run
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

To reload gesture profiles after editing:
```bash
touchctl reload
```

---

## License
MIT License. See [LICENSE](./LICENSE) for details.