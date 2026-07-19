# Global instructions

## Coding style

- Use Allman-style brackets (opening brace on its own line) for methods, structs, enums, `if`, and `for` loops.
- Keep comments light. Follow standard Rust commenting: a brief description of intent along with its parameter info + return type info, plus explicit safety-concern notations (e.g. `// SAFETY:` on `unsafe` blocks). Comments should only be for global vars, structs, methods, impl, etc... there should be no comment inside of a method.
- When making methods, private style methods should always be placed under public methods.
- There should be 2 indents / free lines between each method, this also includes the method comment, this way to make it more spacey.
- Avoid using multiple wrapper methods for small operations as this just lengthens the trail needed to follow to understand things.
- When making a new method, make sure it doesn't already exist, if so prompt the user where at and for actions to take.
- When creating helper methods, see if a suitable type already exists in the language's standard lib.
- Private helper related methods should have basic error prints in instances where an error may occur.
- Methods with many parameters should still be single lined, do not indent them.
- When using print methods that may have many parameters, do not indent them, keep it single lined.
- When writting methods with multiple params, there should be a single space after each comma before leading to the new param.
- Try to avoid indenting in statements when there is multiple conditions or the .ok, .iter, .map, .max, etc..
- When multiple statements or conditional ops are needed, put an indent between each one so it's better to read.

## Coding Security and Performance

- Large datasets, make sure they are properly stored, and free'd when needed.
- Prefer performance and memory safety.

## Platform

This project targets Windows only. Do not add cross-platform guards - omit `#[cfg(windows)]` / `#[cfg(not(windows))]` attributes and non-Windows fallback stubs. Assume the Windows API (via `windows-sys`) is always available.

## Creating files

Prompt the user for confirmation before creating any new file. Describe the file's path and purpose, and wait for approval before writing it. Editing existing files does not require this prompt.