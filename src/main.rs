#[macro_use]
extern crate serde_derive;

extern crate gumdrop;
extern crate i3ipc;
extern crate serde_json;
extern crate serde;

use std::env;
use std::error::Error;
use std::collections::VecDeque;
use std::fs;
use std::net::Shutdown;
use std::path::Path;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;

//use clap::{App, Arg, SubCommand};
use gumdrop::Options;
use i3ipc::{I3Connection, I3EventListener, Subscription};
use i3ipc::event::Event;
use i3ipc::event::inner::WindowChange;
use serde::Deserialize;

static BUFFER_SIZE: usize = 100;

fn socket_filename() -> String {
    env::var("HOME").unwrap() + "/.local/share/i3-focus-last.sock"
}

/// Commands sent for client-server interfacing
#[derive(Serialize, Deserialize, Debug)]
enum Cmd {
    SwitchTo(usize),
    GetHistory,
}

fn focus_nth(windows: &VecDeque<i64>, n: usize) -> Result<(), Box<dyn Error>> {
    let mut conn = I3Connection::connect().unwrap();
    let mut k = n;

    // Start from the nth window and try to change focus until it succeeds
    // (so that it skips windows which no longer exist)
    while let Some(wid) = windows.get(k) {
        let r = conn.run_command(format!("[con_id={}] focus", wid).as_str())?;

        if let Some(o) = r.outcomes.get(0) {
            if o.success {
                return Ok(());
            }
        }

        k += 1;
    }

    Err(From::from(format!("Last {}nth window unavailable", n)))
}

fn cmd_server(windows: Arc<Mutex<VecDeque<i64>>>) {
    let socket = socket_filename();
    let socket = Path::new(&socket);

    if socket.exists() {
        fs::remove_file(&socket).unwrap();
    }

    // Listen to client commands
    let listener = UnixListener::bind(socket).unwrap();

    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let winc = Arc::clone(&windows);

            thread::spawn(move || {
                //let cmd = serde_json::from_reader::<_, Cmd>(&stream);
                let mut de = serde_json::Deserializer::from_reader(&stream);
                let cmd = Cmd::deserialize(&mut de);

                match cmd {
                    Ok(Cmd::SwitchTo(n)) => {
                        let winc = winc.lock().unwrap();

                        // This can fail, that's fine
                        focus_nth(&winc, n).ok();
                    }
                    Ok(Cmd::GetHistory) => {
                        let winc = winc.lock().unwrap();
                        let _ = serde_json::to_writer(&stream, &*winc);
                    }
                    _ => {
                        let _ = serde_json::to_writer(&stream, "invalid command");
                    }
                }
                let _ = stream.shutdown(Shutdown::Both);
            });
        }
    }
}

fn get_focused_window() -> Result<i64, ()> {
    let mut conn = I3Connection::connect().unwrap();
    let mut node = conn.get_tree().unwrap();

    while !node.focused {
        let fid = node.focus.into_iter().next().ok_or(())?;
        node = node.nodes.into_iter().find(|n| n.id == fid).ok_or(())?;
    }

    Ok(node.id)
}

fn focus_server() {
    let mut listener = I3EventListener::connect().unwrap();
    let windows = Arc::new(Mutex::new(VecDeque::new()));
    let windowsc = Arc::clone(&windows);

    // Add the current focused window to bootstrap the list
    get_focused_window().map(|wid| {
        let mut windows = windows.lock().unwrap();

        windows.push_front(wid);
    }).ok();

    thread::spawn(|| cmd_server(windowsc));

    // Listens to i3 event
    let subs = [Subscription::Window];
    listener.subscribe(&subs).unwrap();

    for event in listener.listen() {
        match event.unwrap() {
            Event::WindowEvent(e) => {
                if let WindowChange::Focus = e.change {
                    let mut windows = windows.lock().unwrap();

                    // dedupe, push front and truncate
                    windows.retain(|v| *v != e.container.id);
                    windows.push_front(e.container.id);
                    windows.truncate(BUFFER_SIZE);
                }
            }
            _ => unreachable!(),
        }
    }
}

fn focus_client(nth_window: usize) {
    let mut stream = UnixStream::connect(socket_filename()).unwrap();

    // Just send a command to the server
    serde_json::to_vec(&Cmd::SwitchTo(nth_window))
        .map(move |b| stream.write_all(b.as_slice()))
        .ok();
}

#[derive(Debug, Options)]
enum Command {
    #[options(help = "switch")]
    Switch(SwitchOpts),
    #[options(help = "start server")]
    Server(ServerOpts),
}

#[derive(Debug, Options)]
struct SwitchOpts {
    #[options(help = "nth window to focus", no_long, short = "n", default = "1")]
    count: usize,
}

#[derive(Debug, Options)]
struct ServerOpts {}

#[derive(Debug, Options)]
struct ProgOptions {
    #[options(help = "help")]
    help: bool,

    #[options(help = "version")]
    version: bool,

    #[options(command)]
    command: Option<Command>,
}


fn main() {
    let opts = ProgOptions::parse_args_default_or_exit();

    if opts.version {
        println!("i3-focus-last {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    match opts.command {
        Some(Command::Server(_)) => { focus_server(); }
        Some(Command::Switch(o)) => { focus_client(o.count); }
        _ => { focus_client(1); }
    }
}
