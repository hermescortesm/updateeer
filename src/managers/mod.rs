//! The package managers: finding what is outdated or installed, and running the updates.
//! `scan` and `upgrade` are the entry points; the rest is one backend per tool or OS.
pub mod catalog;
pub mod discover;
pub mod linuxpkg;
pub mod local;
pub mod mas;
pub mod masroot;
pub mod scan;
pub mod upgrade;
pub mod winpkg;
