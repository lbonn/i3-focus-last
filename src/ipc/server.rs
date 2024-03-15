use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use serde::de::Deserialize;

use gumdrop::Options;

use swayipc::{Connection, EventType};
use swayipc::{Event, WindowChange};

use crate::ipc::{socket_filename, Cmd};
use crate::utils;

static BUFFER_SIZE: usize = 100;

#[derive(Debug, Options)]
pub struct ServerOpts {}

fn focus_nth(windows: &[i64], n: usize) -> Result<(), Box<dyn Error>> {
    let mut conn = Connection::new()?;
    let mut k = n;

    // Start from the nth window and try to change focus until it succeeds
    // (so that it skips windows which no longer exist)
    while let Some(wid) = windows.get(k) {
        let r = conn.run_command(format!("[con_id={}] focus", wid).as_str())?;

        if let Some(o) = r.first() {
            if o.is_ok() {
                return Ok(());
            }
        }

        k += 1;
    }

    Err(From::from(format!("Last {}nth window unavailable", n)))
}

fn cmd_server(
    exit: mpsc::Receiver<()>,
    windows: Arc<Mutex<VecDeque<i64>>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket = socket_filename()?;
    let socket = Path::new(&socket);

    if socket.exists() {
        fs::remove_file(socket)?;
    }

    // zip exit and stream messages (client commands) into one channel
    let (stream_tx, stream_rx) = mpsc::channel::<Option<UnixStream>>();

    let stream_tx_ex = stream_tx.clone();
    thread::spawn(move || {
        exit.recv().ok();
        stream_tx_ex.send(None).ok();
    });

    let listener = UnixListener::bind(socket)?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            stream_tx.send(Some(stream)).ok();
        }
    });

    while let Ok(stream) = stream_rx.recv() {
        let windows = Arc::clone(&windows);
        if stream.is_none() {
            // we got an exit cmd
            break;
        }
        let mut stream = stream.unwrap();

        thread::spawn(move || {
            let mut de = serde_json::Deserializer::from_reader(&stream);
            let cmd = Cmd::deserialize(&mut de);

            match cmd {
                Ok(Cmd::SwitchTo(n)) => {
                    // work on a copy to only keep the lock as needed
                    let windows = Vec::from_iter((*windows.lock().unwrap()).iter().cloned());

                    // This can fail, that's fine
                    focus_nth(&windows, n).ok();
                }
                Ok(Cmd::GetHistory) => {
                    let v = {
                        let windows = Vec::from_iter((*windows.lock().unwrap()).iter().cloned());
                        serde_json::to_vec::<Vec<_>>(&windows).unwrap()
                    };
                    let _ = &stream.write(&v);
                }
                _ => {
                    let _ = serde_json::to_writer(&stream, "invalid command");
                }
            }
            let _ = stream.shutdown(Shutdown::Both);
        });
    }

    Ok(())
}

/// Run the focus server that answers clients using the IPC
pub fn focus_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut conn = Connection::new()?;
    let windows = Arc::new(Mutex::new(VecDeque::new()));
    let windowsc = Arc::clone(&windows);

    // Add the current focused window to bootstrap the list
    utils::get_focused_window(&conn.get_tree()?)
        .map(|wid| {
            let mut windows = windows.lock().unwrap();

            windows.push_front(wid);
        })
        .ok();

    // Listens to i3 event
    let events = conn.subscribe([EventType::Window])?;

    let (server_exit_tx, server_exit_rx) = mpsc::channel::<()>();
    let server_handle = thread::spawn(move || cmd_server(server_exit_rx, windowsc));

    for event in events {
        if let Err(_e) = event {
            break;
        }

        if let Event::Window(e) = event.unwrap() {
            match e.change {
                WindowChange::Focus => {
                    let mut windows = windows.lock().unwrap();
                    let cid = e.container.id;

                    // dedupe, push front and truncate
                    windows.retain(|v| *v != cid);
                    windows.push_front(cid);
                    windows.truncate(BUFFER_SIZE);
                }
                WindowChange::Close => {
                    let mut windows = windows.lock().unwrap();
                    let cid = e.container.id;

                    // remove
                    windows.retain(|v| *v != cid);
                }
                _ => {}
            }
        }
    }

    server_exit_tx.send(()).ok();
    server_handle.join().unwrap()?;

    Ok(())
}
