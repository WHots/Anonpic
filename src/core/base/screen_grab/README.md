# Screen grab module

This directory implements interactive region selection. It coordinates the Windows overlay with the shared screenshot and save layers; it does not own image encoding or metadata cleanup.

## Files

| File | Responsibility |
| --- | --- |
| `free_roam_screen_grab.rs` | Public capture entry points and the high-level flow: load settings, optionally freeze the desktop, request a region, crop or capture it, and pass it to the save layer. |
| `overlay.rs` | Overlay window creation and teardown, thread-local selection state, the Win32 message loop, and mouse/keyboard input handling. |
| `drawing.rs` | All overlay GDI rendering, including the reusable back buffer, frozen desktop, selection outlines, and live size label. |
| `math_utils.rs` | Deterministic conversion and geometry helpers for mouse coordinates, normalized rectangles, and bounded circular selections. |
| `mod.rs` | Declares the public capture module and its private implementation modules. |
| `AGENTS.md` | Local maintenance rules that preserve these boundaries during future changes. |

The actual desktop bitmap capture and cropping implementation remains in `src/core/helpers/graphics/screen_capture.rs`. Saved-image encoding, clipboard handling, metadata processing, and filesystem behavior remain under `src/core/base/saves`.

## Capture flow

1. `free_roam_screen_grab` reads the current configuration and virtual desktop dimensions.
2. When freeze mode is enabled, `Screenshot` captures the complete virtual desktop before the overlay appears.
3. `overlay` runs the selection window and delegates every paint request to `drawing`.
4. `math_utils` converts the committed drag into a rectangular or circular capture region.
5. `free_roam_screen_grab` crops the frozen frame or captures the live region, then hands the result to `user_saves`.

The borrowed frozen `HBITMAP` must remain owned by its `Screenshot` for the entire overlay message loop. Drawing code may select that handle into temporary device contexts, but it must never delete it.
