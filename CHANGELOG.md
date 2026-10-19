# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* Core domain models, validation, and sync payload types (`termru-core`).
* SQLite persistence with migrations, backup/restore, soft-delete tombstones
  (`termru-store`).
* SSH engine on russh: auth (password/key/agent), known-hosts with TOFU and
  changed-key detection, PTY sessions with reconnect policy (`termru-ssh`).
* SSH integration test suite against a real OpenSSH server.
* Architecture decision records under `docs/architecture/`.
* Desktop app scaffold: Tauri 2 + Svelte 5 + Vite 8 + TypeScript 5 + xterm 6.
