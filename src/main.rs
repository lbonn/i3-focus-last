#[macro_use]
extern crate serde_derive;

extern crate gumdrop;
extern crate i3ipc;
extern crate serde_json;
extern crate serde;

use std::env;
use std::error::Error;
use std::collections::VecDeque;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::net::Shutdown;
use std::path::Path;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::str::from_utf8;
use std::thread;

use gumdrop::Options;
use i3ipc::{I3Connection, I3EventListener, Subscription};
use i3ipc::reply::{Node,NodeType};
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

fn extract_windows(root: &Node) -> HashMap<i64, &Node> {
    let mut out = HashMap::new();

    let mut expl = VecDeque::new();
    expl.push_front(root);
    while let Some(e) = expl.pop_front() {
        if e.nodetype == NodeType::Con && e.nodes.is_empty() && e.floating_nodes.is_empty() {
            out.insert(e.id, e);
            continue;
        }

        for c in &e.nodes {
            expl.push_front(&c);
        }
        for c in &e.floating_nodes {
            expl.push_front(&c);
        }
    }

    out
}

fn get_focus_history() -> Result<Vec<i64>, Box<dyn Error>> {
    let mut stream = UnixStream::connect(socket_filename())?;

    // Just send a command to the server
    let out = serde_json::to_vec(&Cmd::GetHistory)
        .map(move |b| {
                stream.write_all(b.as_slice()).unwrap();
                let mut de = serde_json::Deserializer::from_reader(&stream);
                Vec::deserialize(&mut de)
            })??;
    Ok(out)
}

fn html_escape(instr: &str) -> String {
    instr.chars()
        .map(|c| match c {
            '&' => "&amp;".chars().collect(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&#39;".chars().collect(),
            _ => vec!(c),
        }).flatten().collect()
}

fn window_format_line(node: &Node) -> String {
    // TODO: add marks
    // TODO: nicer format
    format!("{}\n", html_escape(node.name.as_ref().unwrap_or(&" ".to_string())))
}

fn choose_with_menu(menu: &str, windows: &[&Node]) -> Option<usize> {
    // TODO: better split
    let cmd: Vec<&str> = menu.split(' ').collect();

    let mut child = Command::new(cmd[0])
        .args(cmd[1 .. ].iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to launch menu");
    {
        let stdin = child.stdin.as_mut().expect("stdin!");
        for w in windows {
            let line = window_format_line(&w);
            stdin.write_all(line.as_bytes()).expect("");
        }
    }

    let out = child.wait_with_output().expect("");
    let s = from_utf8(out.stdout.as_slice()).unwrap();
    let s : String = s.chars().filter(|x| match x {
            ' ' | '\n' => false,
            _ => true,
    }).collect();

    s.parse().ok()
}

fn focus_menu(menu: String) {
    let mut conn = I3Connection::connect().unwrap();

    let t = conn.get_tree().unwrap();
    let ws = extract_windows(&t);

    let mut hist = get_focus_history().unwrap_or_default();
    let mut ordered_windows: Vec<&Node> = vec!();
    let mut removed = HashSet::new();
    if !hist.is_empty() {
        hist.remove(0);
    }
    for i in hist {
        if let Some(n) = ws.get(&i) {
            ordered_windows.push(*n);
            removed.insert(i);
        }
    }
    for (i, w) in ws {
        if !removed.contains(&i) {
            ordered_windows.push(w);
        }
    }

    if let Some(choice) = choose_with_menu(&menu, &ordered_windows) {
        let wid = ordered_windows[choice].id;
        conn.run_command(format!("[con_id={}] focus", wid).as_str()).unwrap();
    }
}

#[derive(Debug, Options)]
enum ProgCommand {
    #[options(help = "switch")]
    Switch(SwitchOpts),
    #[options(help = "start server")]
    Server(ServerOpts),
    #[options(help = "start menu")]
    Menu(MenuOpts),
}

#[derive(Debug, Options)]
struct SwitchOpts {
    #[options(help = "nth window to focus", no_long, short = "n", default = "1")]
    count: usize,
}

#[derive(Debug, Options)]
struct ServerOpts {}

#[derive(Debug, Options)]
struct MenuOpts {
    #[options(help = "menu to run", default = "rofi -dmenu -matching fuzzy -markup-rows -i -p window -format i")]
    menu: String,
}

#[derive(Debug, Options)]
struct ProgOptions {
    #[options(help = "help")]
    help: bool,

    #[options(help = "version")]
    version: bool,

    #[options(command)]
    command: Option<ProgCommand>,
}


fn main() {
    let opts = ProgOptions::parse_args_default_or_exit();

    if opts.version {
        println!("i3-focus-last {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    match opts.command {
        Some(ProgCommand::Server(_)) => { focus_server(); }
        Some(ProgCommand::Switch(o)) => { focus_client(o.count); }
        Some(ProgCommand::Menu(m)) => { focus_menu(m.menu); }
        _ => { focus_client(1); }
    }
}
