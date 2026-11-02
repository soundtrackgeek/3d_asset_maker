//! Shared engine for the TRELLIS.2 Studio GUI and the headless `t2` CLI.
pub mod doctor;
pub mod env;
pub mod job;
pub mod mock;
pub mod options;
pub mod paths;
pub mod runner;
pub mod setup;
pub mod update;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = "soundtrackgeek/3d_asset_maker";
