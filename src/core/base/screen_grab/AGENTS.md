# Screen grab maintenance instructions

These instructions apply to this directory in addition to the repository-level `AGENTS.md`.

## Module boundaries

- Keep `free_roam_screen_grab.rs` focused on configuration, capture orchestration, and delegation to the save layer.
- Keep Win32 window creation, message dispatch, selection state, and input handling in `overlay.rs`.
- Keep GDI painting and owned drawing resources in `drawing.rs`.
- Keep deterministic coordinate and region calculations in `math_utils.rs`; add unit tests there when geometry behavior changes.
- Reuse `crate::core::helpers::graphics::screen_capture::Screenshot` for desktop capture and cropping. Do not duplicate bitmap-capture logic in this directory.
- Keep helper modules private unless another subsystem has a demonstrated need for their API.

## Win32 safety

- Preserve overlay thread affinity: window state and the message loop must remain on the thread that creates the overlay.
- Treat the frozen `HBITMAP` as borrowed from `Screenshot`; never delete it from overlay or drawing code.
- Restore every selected GDI object before deleting its owner and release every created GDI handle exactly once.
- Keep a `// SAFETY:` explanation on every `unsafe` block and document unexpected Win32 failures with a basic error message.

## Documentation and verification

- Update this directory's `README.md` whenever a file's responsibility or the capture flow changes.
- Preserve the repository's Allman brace style manually; do not run a repository-wide formatter that rewrites unrelated files.
- Run `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo check --release --manifest-path src-tauri/Cargo.toml`, and `git diff --check` after structural changes.
