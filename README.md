<p align="center">
  <img src="ui/logo.png" alt="Anonpic logo" width="160" />
</p>

<h1 align="center">Anonpic</h1>

<p align="center">
  <em>Capture a region of your screen, strip its identifying metadata, and save or copy a clean image.</em>
</p>

<p align="center">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows-0078D6?logo=windows&logoColor=white" />
  <img alt="Backend" src="https://img.shields.io/badge/backend-Rust-000000?logo=rust&logoColor=white" />
  <img alt="Framework" src="https://img.shields.io/badge/framework-Tauri%20v2-24C8DB?logo=tauri&logoColor=white" />
</p>

> [!NOTE]
> **Built with AI assistance.** This project was developed with the help of AI tools, primarily **Claude Code**, and **Codex** used for one or two prompts.
> - **Backend (Rust):** AI-assisted, but every part was **reviewed and structured by a human**.
> - **Testing (User):** All testing was done manually by a human.
> - **Frontend (UI: HTML / CSS / JS):** written **100% by Claude Code, maybe Codex i can't remember but mostly Claude Code was used**.
> - **README.md (This):** 99% of this readme was generated via AI, Claude Code... Without the rocket ship :)**


---

## What is Anonpic?

Anonpic is a Windows desktop app for taking **privacy-clean screenshots**. You drag-select any region of your screen, and Anonpic saves the result as an image with its **EXIF and authoring metadata stripped**. so the file you share carries no camera, GPS, software, author, or host-machine fingerprints. It can also copy the cleaned image straight to your clipboard.

It is built with **Tauri v2** (Rust backend + WebView UI) and talks to the OS directly through Microsoft's official [`windows-sys`](https://crates.io/crates/windows-sys) bindings — GDI/GDI+ for capture and encoding, a low-level keyboard hook for the hotkey, and native toast notifications.

## Features

- 📸 **Region capture** — press <kbd>Print Screen</kbd> (a global hotkey) or click **Capture an area now** to bring up a dimmed full-screen overlay and drag out the area you want.
- 📐 **Live size readout** — a `width × height` label follows the cursor while you drag, on a flicker-free double-buffered overlay. Right-click or <kbd>Esc</kbd> cancels.
- 🧼 **Automatic metadata scrubbing** — every saved image has its EXIF and common authoring metadata removed before it ever hits disk.
- 📋 **Clipboard copy** — optionally place the cleaned image on the clipboard as a device-independent bitmap, so it pastes into any app and survives Anonpic closing.
- 💾 **Flexible saving** — auto-save to your Images folder, copy to clipboard, or both (independent toggles).
- 🎲 **Random file names** — saved files get an unguessable, cryptographically random name (see below).
- 🖼️ **Format choice** — save as **PNG**, **JPEG**, or **BMP**.
- 🔔 **Native toasts** — a Windows notification confirms each save / clipboard copy.

## How it works

1. **Trigger** — the global <kbd>Print Screen</kbd> hook (or the in-app button) starts a capture.
2. **Snapshot** — the entire virtual desktop is snapshotted up front, *then* the dimmed overlay is shown, so the overlay itself never appears in your screenshot.
3. **Select** — you drag a rectangle; the live `W × H` size is drawn next to the cursor.
4. **Crop & clean** — the chosen region is cropped from the snapshot, encoded to your chosen format, and run through the EXIF + metadata strippers.
5. **Dispatch** — depending on your settings, the cleaned image is saved to disk, copied to the clipboard, or both, and a toast confirms it.

## Privacy: what gets removed

Captured images are encoded with GDI+ and then passed through two strippers. For **JPEG** files the EXIF segments are scrubbed directly (lossless); for other formats the image is re-encoded with all property items removed.

### EXIF (camera & location) — `xif_data`

All EXIF property items are removed. The privacy-relevant points specifically recognized include:

| Field | EXIF tag |
| --- | --- |
| Camera make | `0x010F` |
| Camera model | `0x0110` |
| Software | `0x0131` |
| Original date/time | `0x9003` |
| Orientation | `0x0112` |
| Pixel X / Y dimensions | `0xA002` / `0xA003` |
| GPS latitude (+ ref) | `0x0002` / `0x0001` |
| GPS longitude (+ ref) | `0x0004` / `0x0003` |
| GPS altitude (+ ref) | `0x0006` / `0x0005` |

### Authoring & "Details" tab metadata — `metadata`

| Field | Tag |
| --- | --- |
| Document name | `0x010D` |
| Image description | `0x010E` |
| Software | `0x0131` |
| Date/time | `0x0132` |
| Artist | `0x013B` |
| Host computer | `0x013C` |
| Copyright | `0x8298` |
| XP Title | `0x9C9B` |
| XP Comment | `0x9C9C` |
| XP Author | `0x9C9D` |
| XP Keywords | `0x9C9E` |
| XP Subject | `0x9C9F` |

### How file names are generated

Saved files are named with a **cryptographically random** string so the name leaks nothing about the content or capture time:

- Random bytes come from the OS CSPRNG via `BCryptGenRandom`.
- The name is **8–14 characters** drawn from a filename-safe set: `A–Z`, `a–z`, `0–9`, and `! @ # $ % ^ & ( ) - _ = + [ ] { }`.
- The extension matches the chosen format — e.g. `Xy7$kQ2m.png`.

## Settings

The **Settings** tab persists to `config/app.cfg` and controls:

| Setting | Description |
| --- | --- |
| **Save directory** | Where cleaned images are written. Defaults to an `Images` folder. |
| **Image format** | `PNG`, `JPEG`, or `BMP`. |
| **Copy to clipboard** | After a capture, copy the cleaned image to the clipboard. |
| **Auto-save to Images folder** | Keep the cleaned image on disk. With this off (and clipboard on), the file is used only as a staging step and removed after copying. |

> Both options are independent checkboxes, so you can do either, both, or — if you only want the clipboard — copy without leaving a file behind.

## Project layout

```
.
├── src/                              # Rust backend (bin path is set in src-tauri/Cargo.toml)
│   ├── main.rs                       # Tauri setup, global Print Screen hook, command registration
│   └── core/
│       ├── base/
│       │   ├── configs/              # settings model + config/app.cfg persistence
│       │   ├── notify/               # native Windows toast notifications
│       │   ├── saves/                # save cleaned image + clipboard copy (CF_DIB)
│       │   └── screen_grab/          # free-roam region-capture overlay
│       ├── helpers/
│       │   ├── file_data_operations/ # EXIF + metadata stripping via GDI+
│       │   ├── file_operations/      # filesystem + random filename helpers
│       │   ├── graphics/             # GDI screen capture
│       │   └── windows_*/            # Win32 window helpers
│       └── logic/events/             # global low-level keyboard hook (Print Screen)
├── ui/                               # Frontend (vanilla HTML/CSS/JS) — Claude Code
│   ├── index.html
│   ├── logo.png
│   └── src/{main.js, styles.css}
└── src-tauri/                        # Tauri manifest, config, icons, capabilities
    ├── Cargo.toml                    # [[bin]] path -> ../src/main.rs; windows-sys features
    ├── tauri.conf.json
    ├── capabilities/
    └── icons/
```

## Building & running

> **Windows only.** Anonpic uses the Win32 API throughout and is not designed to be cross-platform.

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- Tauri's [system prerequisites](https://tauri.app/start/prerequisites/) — on Windows: the **WebView2** runtime (preinstalled on Windows 11) and the **Microsoft C++ Build Tools**.
- The Tauri CLI:

  ```sh
  cargo install tauri-cli --version "^2.0.0" --locked
  ```

### Develop

Run the app with hot-reloading:

```sh
cargo tauri dev
```

### Build a release bundle

```sh
cargo tauri build
```

### Type-check the backend only

```sh
cargo check --manifest-path src-tauri/Cargo.toml
```

## Roadmap / TODO

- [ ] **Strip metadata from existing images** — let the user point Anonpic at images they already have (not just new captures) and scrub them in place.
- [ ] **Metadata spoofing** — instead of only stripping, optionally **write custom values** (fake camera, timestamp, GPS, author, etc.) so the user can deliberately spoof an image's data.

## Recommended IDE setup

- [VS Code](https://code.visualstudio.com/) +
  [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) +
  [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
