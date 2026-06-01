//! TinyVPN Core — shared types, crypto, protocol definitions

pub mod config;
pub mod crypto;
pub mod protocol;
pub mod tls;
pub mod wg;

pub use config::NodeConfig;
