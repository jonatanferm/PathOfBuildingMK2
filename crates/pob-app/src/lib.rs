//! `pob-app`: Path of Building itself on the Rust UI port (ui doc §3, §10).
//!
//! Launch, Main, BuildList mode, Build mode, every tab, PassiveTreeView and tree assets, gem/item
//! tooltips, the calc facade (RustCalc; LuaCalcBridge injected in dev) and platform storage/net
//! glue. Module declarations are UI-U0's (§12.1); each file's header names its owning unit.
//! Model and calc types are reached only through [`model`] and [`calc`] (§10.2).
#![allow(
    clippy::new_ret_no_self,
    clippy::module_inception,
    clippy::wildcard_imports,
    clippy::too_many_arguments
)]

pub mod app;
pub mod build;
pub mod build_list;
pub mod calc;
pub mod controls;
pub mod ext_build_list;
pub mod launch;
pub mod main_mode;
pub mod model;
pub mod platform;
pub mod tabs;
pub mod trade;
pub mod tree_view;

pub use app::{app, app_mut, AppState, CxApp, PobApp};
