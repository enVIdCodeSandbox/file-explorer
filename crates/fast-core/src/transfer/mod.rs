//! File transfers: strategy selection, the copy engine and time estimation.

pub mod careful;
pub mod engine;
pub mod eta;
pub mod method;
pub mod probe;
pub mod strategy;
pub mod throttle;

pub use engine::{start, start_with, Conflict, ConflictInfo, Op, Resolution, Phase, Predictor, Progress, TransferHandle, TransferSpec};
pub use eta::{DisplayMode, EtaModel, EtaParams};
pub use strategy::{Plan, Strategy};

