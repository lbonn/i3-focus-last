use serde::{Deserialize, Serialize};
use std::env;

pub mod client;
pub mod server;

pub fn socket_filename() -> Result<String, Box<env::VarError>> {
    Ok(env::var("HOME")? + "/.local/share/i3-focus-last.sock")
}

/// Commands sent for client-server interfacing
#[derive(Serialize, Deserialize, Debug)]
pub enum Cmd {
    SwitchTo(usize),
    GetHistory,
}
