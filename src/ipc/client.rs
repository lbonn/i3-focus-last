use std::error::Error;
use std::os::unix::net::UnixStream;

use std::io::Write;

use serde::de::Deserialize;

use crate::ipc::{socket_filename, Cmd};

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
pub fn get_focus_history() -> Result<Vec<i64>, Box<dyn Error>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    let out =
        serde_json::to_vec(&Cmd::GetHistory).map(move |b| -> Result<_, Box<dyn Error>> {
            stream.write_all(b.as_slice())?;
            let mut de = serde_json::Deserializer::from_reader(&stream);
            let o = Vec::deserialize(&mut de)?;
            Ok(o)
        })??;
    Ok(out)
}
