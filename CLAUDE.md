# Global instructions

## Coding style

- Prefer performance and memory safety.
- Use Allman-style brackets (opening brace on its own line) for methods, structs, enums, `if`, and `for` loops.
- Keep comments light. Follow standard Rust commenting: a brief description of intent along with its parameter info + return type info, plus explicit safety-concern notations (e.g. `// SAFETY:` on `unsafe` blocks). For commenting, comments should only be for global vars, structs, methods, impl, etc... there should be no comment inside of a method.
- when making methods, private style methods should always be placed under public methods.
- There should be 2 indents / free lines between each method, this also includes the method comment, this way to make it more spacey.

## Platform

This project targets Windows only. Do not add cross-platform guards — omit `#[cfg(windows)]` / `#[cfg(not(windows))]` attributes and non-Windows fallback stubs. Assume the Windows API (via `windows-sys`) is always available.

## Creating files

Prompt the user for confirmation before creating any new file. Describe the file's path and purpose, and wait for approval before writing it. (Editing existing files does not require this prompt.)

## Codex Agents integration

When applying changes in a directory, look for an `AGENTS.md` file in that directory (and walk up parent directories). If present, read it and follow its development instructions for any changes scoped to that directory. Treat `AGENTS.md` as authoritative for directory-local conventions alongside any `CLAUDE.md`.

When traversing directories to make changes, reference any `AGENTS.md` files alongside `CLAUDE.md` files.

If any design pattern or instruction in a `CLAUDE.md` file contradicts one in an `AGENTS.md` file (or vice versa), do **not** silently pick one. Stop and:

1. List every specific contradiction to the user, citing both files and quoting the conflicting rules.
2. Ask the user to decide how to proceed, offering three choices:
   - **CLAUDE** — follow the `CLAUDE.md` rule.
   - **AGENTS** — follow the `AGENTS.md` rule.
   - **Other** — the user describes how they want you to proceed instead.
3. Wait for the user's input before making any changes.

## Claude / Agents editing guide.

prompt user before changing anything in the CLAUDE.md or AGENTS.md files, displaying what will be changed, user should have option to confirm or deny.