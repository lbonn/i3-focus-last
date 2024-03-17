use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use signal_hook::consts::*;
use signal_hook::iterator::Signals;

use serde::de::Deserialize;

use gumdrop::Options;

use crate::ipc::{socket_filename, Cmd};
use crate::utils;

static BUFFER_SIZE: usize = 100;

#[derive(Debug, Options)]
pub struct ServerOpts {}

fn focus_nth<'a, I>(windows: I, n: usize) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = &'a i64>,
{
    // Start from the nth window and try to change focus until it succeeds
    // (so that it skips windows which no longer exist)
    for (k, wid) in windows.into_iter().enumerate() {
        if k < n {
            continue;
        }

        let mut conn = swayipc::Connection::new()?;
        let r = conn.run_command(format!("[con_id={}] focus", wid).as_str())?;

        if let Some(o) = r.first() {
            if o.is_ok() {
                return Ok(());
            }
        }
    }

    Err(From::from(format!("Last window {} unavailable", n)))
}

#[derive(Debug)]
enum ServerEvent {
    I3Event(swayipc::Event),
    SwitchTo(usize),
    GetHistory(mpsc::Sender<Vec<i64>>),
    Stop(Result<(), Box<dyn Error + Send + Sync>>),
}

fn cmd_listener(event_chan: mpsc::Sender<ServerEvent>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket = socket_filename()?;
    let socket = Path::new(&socket);

    if socket.exists() {
        fs::remove_file(socket)?;
    }

    let listener = UnixListener::bind(socket)?;

    for stream in listener.incoming() {
        let mut stream = stream?;
        let event_chan = event_chan.clone();
        thread::spawn(move || {
            let mut de = serde_json::Deserializer::from_reader(&stream);
            let cmd = Cmd::deserialize(&mut de);

            let res = (|| -> Result<(), Box<dyn Error + Send + Sync>> {
                match cmd {
                    Ok(Cmd::SwitchTo(n)) => {
                        event_chan.send(ServerEvent::SwitchTo(n))?;
                    }
                    Ok(Cmd::GetHistory) => {
                        let (hist_tx, hist_rx) = mpsc::channel::<Vec<i64>>();
                        event_chan.send(ServerEvent::GetHistory(hist_tx))?;
                        let windows = hist_rx.recv().unwrap();
                        let v = serde_json::to_vec::<Vec<_>>(&windows).unwrap();
                        let _ = &stream.write(&v);
                    }
                    _ => {
                        let _ = serde_json::to_writer(&stream, "invalid command");
                    }
                }
                Ok(())
            })();

            let _ = stream.shutdown(Shutdown::Both);

            if let Err(err) = res {
                println!("{}", err);
            }
        });
    }

    Ok(())
}

/// Run the focus server that answers clients using the IPC
fn i3events_listener(
    conn: swayipc::Connection,
    event_chan: mpsc::Sender<ServerEvent>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Listens to i3 event
    let events = conn.subscribe([swayipc::EventType::Window])?;

    for event in events {
        let server_ev = match event {
            Ok(ev) => ServerEvent::I3Event(ev),
            Err(err) => ServerEvent::Stop(Err(Box::new(err))),
        };
        event_chan.send(server_ev)?;
    }

    Ok(())
}

fn interrupt_listener(
    event_chan: mpsc::Sender<ServerEvent>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut signals = Signals::new([SIGINT])?;

    for _ in &mut signals {
        event_chan.send(ServerEvent::Stop(Ok(())))?;
    }

    Ok(())
}

fn spawn_fallible<F, T>(f: F, server_chan: mpsc::Sender<ServerEvent>)
where
    F: FnOnce(mpsc::Sender<ServerEvent>) -> Result<T, Box<dyn Error + Send + Sync>>
        + Send
        + 'static,
    T: Send + 'static,
{
    thread::spawn(move || {
        if let Err(e) = f(server_chan.clone()) {
            server_chan.send(ServerEvent::Stop(Err(e))).ok();
        }
    });
}

pub fn focus_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (events_tx, events_rx) = mpsc::channel::<ServerEvent>();
    let mut conn = swayipc::Connection::new()?;

    let mut windows = VecDeque::new();
    utils::get_focused_window(&conn.get_tree()?)
        .map(|wid| {
            windows.push_front(wid);
        })
        .ok();

    // i3 events
    {
        let events_tx = events_tx.clone();
        spawn_fallible(move |evtx| i3events_listener(conn, evtx), events_tx);
    }

    // commands
    {
        let events_tx = events_tx.clone();
        spawn_fallible(cmd_listener, events_tx);
    }

    // interrupts
    {
        let events_tx = events_tx.clone();
        spawn_fallible(interrupt_listener, events_tx);
    }

    for ev in events_rx {
        match ev {
            ServerEvent::I3Event(e) => {
                if let swayipc::Event::Window(e) = e {
                    match e.change {
                        swayipc::WindowChange::Focus => {
                            let cid = e.container.id;

                            // dedupe, push front and truncate
                            windows.retain(|v| *v != cid);
                            windows.push_front(cid);
                            windows.truncate(BUFFER_SIZE);
                        }
                        swayipc::WindowChange::Close => {
                            let cid = e.container.id;

                            // remove
                            windows.retain(|v| *v != cid);
                        }
                        _ => {}
                    }
                }
            }
            ServerEvent::SwitchTo(n) => {
                focus_nth(&windows, n).map_err(|e| println!("{}", e)).ok();
            }
            ServerEvent::GetHistory(chan) => {
                let windows = Vec::from_iter(windows.iter().cloned());
                chan.send(windows)?;
            }
            ServerEvent::Stop(res) => {
                res?;
                break;
            }
        }
    }

    Ok(())
}
