//! Módulo de Comandos Tauri
//!
//! Organiza os comandos em submodulos por funcionalidade.

mod audio;
mod capture_cmd;
mod settings;
mod stream;

// Re-exporta todos os comandos
pub use audio::*;
pub use capture_cmd::*;
pub use settings::*;
pub use stream::*;
