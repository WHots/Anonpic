# Project instructions

## Platform

- Target Windows only and assume `windows-sys` APIs are available.
- Do not add platform guards or non-Windows fallbacks.

## Before editing

- Search for an existing method before adding one. If an equivalent exists, tell the user where it is and ask whether to reuse or change it.
- Prefer suitable Rust standard-library types and helpers over custom ones.
- Ask for confirmation before creating a file. State its path and purpose; existing files may be edited without confirmation.

## Rust style

- Use Allman braces for methods, structs, enums, `if`, and `for` blocks.
- Place public methods before related private methods. Leave two blank lines between method blocks, including their documentation.
- Keep method signatures, ordinary argument lists, and print calls on one line. Closure and macro bodies may span lines.
- Separate arguments with one space after each comma and omit the trailing comma.
- Keep simple chains and multi-condition expressions compact. Builder chains containing closures may span lines.
- Keep comments sparse and attach them to items such as globals, structs, methods, and `impl` blocks. Do not comment inside method bodies except for required safety notes.
- Document an item's intent, parameters, and return value briefly. Mark every `unsafe` block with a `// SAFETY:` explanation.
- When a private helper handles an unexpected failure locally, print a basic error message. Expected absence and normal control-flow results need not log.

## Design, safety, and performance

- Prefer direct, simple implementations; avoid unnecessary wrappers and over-engineering.
- Keep structural types in the appropriate module file.
- Prioritize memory safety and performance. Store large datasets efficiently and release resources when no longer needed.


## Documentation

- When changes are made that are associated with README.md anywhere, update that README  accordingly.
