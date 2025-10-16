# Rust Batch Toolchain

Rust based toolchain for Windows Batch (`.bat`) scripting.

This is a open source project, including a **syntax highlighter**, **step-by-step debugger**, and **linter**.

---

## Project Overview

The projects primary goal is quite simple. It's primary job is to:

- Execute `.bat` scripts with **full control-flow tracking**.
- Provide an **interactive debugger** (step, breakpoints, call stack).
- Support **syntax highlighting** and **linting** for Batch files.
- Integrated as a **VS Code extension**.

---

## Current Capabilities

As i am the lone developer of this, in the few hours i have each day, it is obviously not a finished product, but rather something i'm working on as i go. So far i have the following capabilities setup:

- Persistent `cmd.exe` process with piped I/O.
- Safe sentinel-based detection of command completion.
- Label scanning (`:label`) and `GOTO` jumps.
- Subroutine call handling (`CALL :label`) with a proper return stack.
- Returns from subroutines via `EXIT /B`, `GOTO :EOF`, or end-of-file.
- Comment and empty-line skipping.
- Exit code tracking (`ERRORLEVEL`).

---

## MVP TODO — Batch Debugger Roadmap

This is my checklist of what's complete and what is left until i can reach a **Basic MVP** of the debugger.

### Control Flow

- [x] Label scanning (`:label`)
- [x] `GOTO` handling
- [x] `CALL :label` with `EXIT /B` / `GOTO :EOF` / EOF return
- [x] Jump target normalization (case-insensitive labels, allow spaces)
- [x] Proper top-level termination (EOF with empty/non-empty call stack)
- [ ] `CALL external.bat` (decide whether to intercept or delegate)

### Parsing & Normalization

- [ ] Handle extra whitespace/tabs (`GOTO    foo`, `EXIT /B 5`)
- [ ] Support all comment styles (`REM`, `::`, inline comments)
- [ ] Case-insensitive keywords (`goto`, `GoTo`, etc.)
- [ ] Labels with odd chars/spaces (`:x-y`, `:has spaces`)
- [ ] Composite command lines (`cmd1 & cmd2`, `cmd1 && cmd2`, `cmd1 || cmd2`)

### Subroutine Stack

- [x] Push frame on `CALL :label`
- [x] Pop frame on `EXIT /B` / `GOTO :EOF`
- [x] Return to the popped frame’s `return_pc`
- [ ] Display a readable call-stack in debugger output

### Arguments (`%1..%9`)

- [ ] Parse arguments in `CALL :label arg1 "arg two"`
- [ ] Expose arguments as `%1..%9`, `%*`, implement `SHIFT`
- [ ] Handle quoting/escaping (basic version)

### Variable Expansion & Scope

- [ ] `%VAR%` vs `!VAR!` delayed expansion awareness
- [ ] `SETLOCAL` / `ENDLOCAL` environment scopes
- [ ] Track or mirror `ERRORLEVEL` in debugger view

### Blocks, Conditionals & Loops

- [ ] `IF` statements (string compare, `ERRORLEVEL`, `EXIST`, `DEFINED`)
- [ ] Parenthesized blocks (`IF (...) ELSE (...)`)
- [ ] `FOR` loops (simple iteration)
- [ ] Line continuation (`^`)

### Redirection & Pipes

- [x] Merge `stderr` → `stdout` (via `2>&1`)
- [ ] Handle pipes (`|`) and redirections (`>`, `>>`, `<`) gracefully
- [ ] Decide stepping behavior for compound commands

### Environment & Context

- [ ] Persistent working directory tracking
- [ ] Show current directory in debugger status
- [ ] Handle drive switches (`C:`, `cd /d`, etc.)

### Echo & Prompt

- [x] Quiet mode (`/Q`) with custom `PROMPT`
- [ ] Respect `@echo off` / `echo on`

### I/O & Robustness

- [x] Sentinel-based output parsing
- [ ] Use a truly unique sentinel (GUID-style)
- [ ] Timeout or cancel long-running commands
- [x] Proper CRLF handling for Windows

### Error Handling & Messaging

- [x] Clear errors on unknown labels
- [x] Graceful handling for empty stack returns
- [ ] Show `exit code` and stack in debugger output

---

## 🧪 Test Plan

No test plan yet, as i haven't deployed nor considered deploying anything properly yet.
My idea is to cover alot of this before i even release a VSCode Extension (Which also needs to be developed, probably as a seperate repository, linked up to this one)

---
