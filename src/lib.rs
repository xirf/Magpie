//! Magpie library crate.
//! Shared by the magpie daemon binary and the magpie-cli utility binary.
pub mod aria2;
pub mod config;
pub mod db;
pub mod discord;
pub mod i18n;
pub mod qbittorrent;
pub mod redis_client;
pub mod s3;
pub mod server;
pub mod tasks;
pub mod telegram;
pub mod torrent_client;
pub mod transmission;
pub mod utils;

pub mod mock_client;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;