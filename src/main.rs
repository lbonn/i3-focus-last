#[macro_use]
extern crate serde_derive;

extern crate clap;
extern crate i3ipc;
extern crate serde_json;
extern crate serde;

use std::env;
use std::error::Error;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;

use clap::{App, Arg, SubCommand};
use i3ipc::{I3Connection, I3EventListener, Subscription};
use i3ipc::event::Event;
use i3ipc::event::inner::WindowChange;

static BUFFER_SIZE: usize = 100;

fn socket_filename() -> String {
    env::var("HOME").unwrap() + "/.local/share/i3-focus-last.sock"
}

/// Commands sent for client-server interfacing
#[derive(Serialize, Deserialize, Debug)]
enum Cmd {
    SwitchTo(usize),
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
                let cmd = serde_json::from_reader::<_, Cmd>(stream);

                if let Ok(Cmd::SwitchTo(n)) = cmd {
                    let winc = winc.lock().unwrap();

                    // This can fail, that's fine
                    focus_nth(&winc, n).ok();
                }
            });
        }
    }
}

fn get_focused_window() -> Result<i64, ()> {
    let mut conn = I3Connection::connect().unwrap();
    let mut node = conn.get_tree().unwrap();

    while !node.focused {
        let fid = node.focus.into_iter().nth(0).ok_or(())?;
        node = node.nodes.into_iter().filter(|n| n.id == fid).nth(0).ok_or(())?;
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

fn main() {
    let matches = App::new("i3-focus-last")
                          .subcommand(SubCommand::with_name("server")
                                     .about("Run in server mode"))
                          .version(env!("CARGO_PKG_VERSION"))
                          .arg(Arg::with_name("nth_window")
                              .short("n")
                              .value_name("N")
                              .help("nth window to focus")
                              .default_value("1")
                              .validator(|v| {
                                  v.parse::<usize>()
                                      .map_err(|e| String::from(e.description()))
                                      .and_then(|v| if v > 0 && v <= BUFFER_SIZE { Ok(v) }
                                                else { Err(String::from("invalid n")) }
                                               )
                                      .map(|_| ())
                              }))
                          .get_matches();

    if matches.subcommand_matches("server").is_some() {
        focus_server();
    } else {
        focus_client(matches.value_of("nth_window").unwrap().parse().unwrap());
    }
}
