//! Library surface for `pentair-daemon`, exposing the **pure** model modules so
//! other workspace crates (notably `pentair-cli`) can reuse them without
//! reimplementing the physics.
//!
//! Only the pure, side-effect-free pieces are re-exported here:
//! [`thermal`] (the forward model), [`scheduler`] (the advisory comfort
//! scheduler + backtest harness), and the config/weather *types* they consume.
//! The daemon binary keeps its own module tree in `main.rs`; this lib exists so
//! the evaluation harness (`scheduler::run_backtest`) can be driven from the CLI.
//!
//! # HARD CONSTRAINT — advisory only
//!
//! Nothing exported here actuates. [`scheduler`] computes and reports a
//! recommended plan + projected energy/cost; it never POSTs `/api/*/heat` or
//! `/on`, never writes a setpoint, and never issues a pump/heater command.

pub mod config;
pub mod scenes;
pub mod scheduler;
pub mod calibrator;
pub mod thermal;
pub mod weather;
