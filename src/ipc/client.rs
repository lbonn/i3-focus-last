use std::error::Error;
use std::os::unix::net::UnixStream;

use std::io::Write;

use crate::ipc::{Cmd, socket_filename};

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
