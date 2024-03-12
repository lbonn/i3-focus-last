use std::error::Error;
use std::os::unix::net::UnixStream;

use std::io::Write;

use crate::ipc::{Cmd, socket_filename};
use serde::de::Deserialize;

/// Focus the last nth window
///
/// Commonly called with `nth_window=1`
pub fn focus_nth_last_client(nth_window: usize) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    serde_json::to_vec(&Cmd::SwitchTo(nth_window))
        .map(move |b| stream.write_all(b.as_slice()))
        .ok();

    Ok(())
}

/// Get the recently focused window IDs
pub fn get_focus_history() -> Result<(Vec<i64>, bool), Box<dyn Error>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    let out =
        serde_json::to_vec(&Cmd::GetHistory).map(move |b| -> Result<_, Box<dyn Error>> {
            stream.write_all(b.as_slice())?;
            let o = serde_json::from_reader::<_, (Vec<i64>, bool)>(&stream)?;
            Ok(o)
        })??;
    Ok(out)
}

pub fn push_to_history(node: i64) -> Result<(), Box<dyn Error>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    serde_json::to_vec(&Cmd::PushToHistory(node))
        .map(move |b| stream.write_all(b.as_slice()))
        .ok();

    Ok(())
}

pub fn take_inhibit_history(lease: Option<u64>) -> Result<u64, Box<dyn Error>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    let out = serde_json::to_vec(&Cmd::InhibitHistory(lease)).map(
        move |b| -> Result<_, Box<dyn Error>> {
            stream.write_all(b.as_slice())?;
            let mut de = serde_json::Deserializer::from_reader(&stream);
            let o = u64::deserialize(&mut de)?;
            Ok(o)
        },
    )??;

    Ok(out)
}

pub fn release_inhibit_history(lease: u64) -> Result<(), Box<dyn Error>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    serde_json::to_vec(&Cmd::InhibitHistoryRelease(lease))
        .map(move |b| stream.write_all(b.as_slice()))
        .ok();

    Ok(())
}
