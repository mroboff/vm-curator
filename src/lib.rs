#![warn(clippy::all)]
//! # vm-curator
//!
//! Core library for [vm-curator](https://github.com/mroboff/vm-curator), a TUI for
//! managing QEMU/KVM virtual machines.
//!
//! This crate exposes the non-UI building blocks so that alternative front-ends
//! (a GUI, a daemon, scripts) can reuse the same VM discovery, launch-script
//! parsing/generation, snapshot, hardware-passthrough, import, and metadata
//! logic that powers the terminal interface.
//!
//! ## Public modules
//!
//! - [`vm`] — VM discovery, launch-script parsing/generation, lifecycle, snapshots, import
//! - [`commands`] — wrappers around `qemu-img` and QEMU system binaries
//! - [`hardware`] — USB / PCI / GPU passthrough enumeration and configuration
//! - [`metadata`] — OS profiles, QEMU profiles, family hierarchy, ASCII art
//! - [`config`] — user settings persisted under `~/.config/vm-curator/`
//! - [`platform`] — OS-specific behavior (acceleration, display, firmware, URL opening)
//! - [`wizard_types`] — front-end-agnostic state types for the creation/import flows
//! - [`fs`] — small filesystem helpers
//!
//! ## Intentionally excluded
//!
//! The `ui` and `app` modules are **not** part of the public API: `ui` contains
//! ratatui/crossterm TUI rendering code, and `app` references `ui` types, so
//! neither can be consumed independently of the terminal front-end.
pub mod commands;
pub mod config;
pub mod fs;
pub mod hardware;
pub mod metadata;
pub mod platform;
pub mod vm;
pub mod wizard_types;
