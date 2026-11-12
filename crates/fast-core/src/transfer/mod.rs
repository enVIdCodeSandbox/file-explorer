//! File transfers: strategy selection, the copy engine and time estimation.

pub mod engine;
pub mod eta;
pub mod strategy;

pub use engine::{start, Conflict, Op, Phase, Predictor, Progress, TransferHandle, TransferSpec};
pub use eta::{DisplayMode, EtaModel, EtaParams};
pub use strategy::{Plan, Strategy};

