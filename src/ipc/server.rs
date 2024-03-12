use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time;

use signal_hook::consts::*;
use signal_hook::iterator::Signals;

use serde::de::Deserialize;

use gumdrop::Options;

use crate::ipc::{Cmd, socket_filename};
use crate::utils;

static BUFFER_SIZE: usize = 100;

#[derive(Debug, Options)]
pub struct ServerOpts {}

fn focus_nth<'a, I>(
    conn: &mut swayipc::Connection,
    windows: I,
    n: usize,
) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = &'a i64>,
{
    // Start from the nth window and try to change focus until it succeeds
    // (so that it skips windows which no longer exist)
    for (k, wid) in windows.into_iter().enumerate() {
        if k < n {
            continue;
        }

        let r = conn.run_command(format!("[con_id={}] focus", wid).as_str())?;

        if let Some(o) = r.first()
            && o.is_ok()
        {
            return Ok(());
        }
    }

    Err(From::from(format!("Last window {} unavailable", n)))
}

struct HistoryInhibitor {
    last_lease_id: u64,
    leases: HashMap<u64, time::Instant>,
}

const LEASE_EXPIRY: time::Duration = time::Duration::from_secs(5);

impl HistoryInhibitor {
    fn new() -> Self {
        let last_lease_id = 0;
        let leases = HashMap::new();

        HistoryInhibitor {
            last_lease_id,
            leases,
        }
    }

    fn inhibited(&self) -> bool {
        !self.leases.is_empty()
    }

    fn inhibit(&mut self, lease: Option<u64>) -> u64 {
        let lease = match lease {
            None => {
                let n = self.last_lease_id;
                self.last_lease_id += 1;
                n
            }
            Some(l) => l,
        };

        self.leases.insert(lease, time::Instant::now());

        lease
    }

    fn release(&mut self, lease: u64) {
        self.leases.remove(&lease);
    }

    fn review_leases(&mut self) {
        let now = time::Instant::now();
        self.leases
            .retain(|_, v| now.duration_since(*v) < LEASE_EXPIRY);
    }
}

#[derive(Debug)]
enum ServerEvent {
    I3Event(swayipc::Event),
    SwitchTo(usize),
    GetHistory(mpsc::Sender<(Vec<i64>, bool)>),
    PushToHistory(i64),
    InhibitHistory(Option<u64>, mpsc::Sender<u64>),
    InhibitHistoryRelease(u64),
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
                        let (hist_tx, hist_rx) = mpsc::channel::<(Vec<i64>, bool)>();
                        event_chan.send(ServerEvent::GetHistory(hist_tx))?;
                        let res = hist_rx.recv()?;
                        let v = serde_json::to_vec::<(Vec<_>, bool)>(&res)?;
                        let _ = &stream.write(&v);
                    }
                    Ok(Cmd::PushToHistory(wid)) => {
                        event_chan.send(ServerEvent::PushToHistory(wid))?;
                    }
                    Ok(Cmd::InhibitHistory(lease)) => {
                        let (tx, rx) = mpsc::channel::<u64>();
                        event_chan.send(ServerEvent::InhibitHistory(lease, tx))?;
                        let lease = rx.recv()?;
                        let v = serde_json::to_vec(&lease)?;
                        let _ = &stream.write(&v);
                    }
                    Ok(Cmd::InhibitHistoryRelease(lease)) => {
                        event_chan.send(ServerEvent::InhibitHistoryRelease(lease))?;
                    }
                    _ => {
                        let _ = serde_json::to_writer(&stream, "invalid command");
                    }
                }
                Ok(())
            })();

            let _ = stream.shutdown(Shutdown::Both);

            if let Err(err) = res {
                eprintln!("error: {}", err);
            }
        });
    }

    Ok(())
}

/// Run the focus server that answers clients using the IPC
fn i3events_listener(
    event_chan: mpsc::Sender<ServerEvent>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let conn = swayipc::Connection::new()?;
    // Listens to i3 event
    let events = conn.subscribe([swayipc::EventType::Workspace, swayipc::EventType::Window])?;

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

fn push_to_history(windows: &mut VecDeque<i64>, winid: i64) {
    windows.retain(|v| *v != winid);
    windows.push_front(winid);
    windows.truncate(BUFFER_SIZE);
}

pub fn focus_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (events_tx, events_rx) = mpsc::channel::<ServerEvent>();

    // i3 events
    {
        let events_tx = events_tx.clone();
        spawn_fallible(i3events_listener, events_tx);
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

    let mut conn = swayipc::Connection::new()?;
    let mut windows = VecDeque::new();
    let mut empty_focus = true;
    utils::get_focused_window(&conn.get_tree()?)
        .map(|wid| {
            windows.push_front(wid);
        })
        .ok();
    let mut hist_inhibitor = HistoryInhibitor::new();

    for ev in events_rx {
        match ev {
            ServerEvent::I3Event(e) => {
                if let swayipc::Event::Window(e) = e {
                    match e.change {
                        swayipc::WindowChange::Focus => {
                            hist_inhibitor.review_leases();
                            if !hist_inhibitor.inhibited() {
                                push_to_history(&mut windows, e.container.id);
                            }
                        }
                        swayipc::WindowChange::Close => {
                            let cid = e.container.id;

                            // remove
                            windows.retain(|v| *v != cid);
                            empty_focus = true;
                        }
                        _ => {}
                    }
                } else if let swayipc::Event::Workspace(e) = e
                    && e.change == swayipc::WorkspaceChange::Focus
                {
                    empty_focus = true;
                }
            }
            ServerEvent::PushToHistory(wid) => {
                push_to_history(&mut windows, wid);
            }
            ServerEvent::InhibitHistory(n, chan) => {
                chan.send(hist_inhibitor.inhibit(n)).ok();
            }
            ServerEvent::InhibitHistoryRelease(n) => {
                hist_inhibitor.release(n);
            }
            ServerEvent::SwitchTo(n) => {
                let n = if empty_focus {
                    std::cmp::max(0, n - 1)
                } else {
                    n
                };
                focus_nth(&mut conn, &windows, n)
                    .map_err(|e| eprintln!("{}", e))
                    .ok();
            }
            ServerEvent::GetHistory(chan) => {
                let windows = Vec::from_iter(windows.iter().cloned());
                chan.send((windows, empty_focus)).ok();
            }
            ServerEvent::Stop(res) => {
                res?;
                break;
            }
        }
    }

    Ok(())
}
