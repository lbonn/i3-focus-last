#[macro_use]
extern crate serde_derive;

extern crate gumdrop;
extern crate swayipc;
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
use swayipc::{Connection, EventType};
use swayipc::{Node, NodeType, Event, WindowChange};

use serde::Deserialize;

static BUFFER_SIZE: usize = 100;
static DEFAULT_ICONS: & [(&str, &str)] = &[
    ("firefox", "firefox"),
    ("Chromium", "chromium"),
];

fn socket_filename() -> String {
    env::var("HOME").unwrap() + "/.local/share/i3-focus-last.sock"
}

/// Commands sent for client-server interfacing
#[derive(Serialize, Deserialize, Debug)]
enum Cmd {
    SwitchTo(SwitchOpts),
    GetHistory,
}

#[derive(Serialize)]
pub struct WinData {
    id: i64,
    scratch: bool,
}

fn focus_nth(windows: &VecDeque<WinData>, opts: SwitchOpts) -> Result<(), Box<dyn Error>> {
    let mut conn = Connection::new().unwrap();
    let mut k = opts.count;

    if opts.hide_scratchpad && get_focused_window().is_err() {
        conn.run_command(format!("scratchpad show").as_str())?;
    }

    // Start from the nth window and try to change focus until it succeeds
    // (so that it skips windows which no longer exist)
    while let Some(win_data) = windows.get(k) {

        if opts.ignore_scratchpad && win_data.scratch {
            k += 1;
            continue;
        }

        let r = conn.run_command(format!("[con_id={}] focus", win_data.id).as_str())?;

        if let Some(o) = r.get(0) {
            if o.is_ok() {
                return Ok(());
            }
        }

        k += 1;
    }

    Err(From::from(format!("Last {}nth window unavailable", opts.count)))
}

fn cmd_server(windows: Arc<Mutex<VecDeque<WinData>>>) {
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
                    Ok(Cmd::SwitchTo(opts)) => {
                        let winc = winc.lock().unwrap();

                        // This can fail, that's fine
                        focus_nth(&winc, opts).ok();
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

fn get_focused_window() -> Result<Node, ()> {
    let mut conn = Connection::new().unwrap();
    let mut node = conn.get_tree().unwrap();

    while !node.focused {
        let fid = node.focus.into_iter().next().ok_or(())?;
        node = node.nodes.into_iter().find(|n| n.id == fid).ok_or(())?;
    }

    Ok(node)
}

fn focus_server(server_opts: ServerOpts) {
    let conn = Connection::new().unwrap();
    let windows = Arc::new(Mutex::new(VecDeque::new()));
    let windowsc = Arc::clone(&windows);
    let mut scratchpad_ids: HashSet<i64> = HashSet::new();

    // Add the current focused window to bootstrap the list
    get_focused_window().map(|node| {
        let mut windows = windows.lock().unwrap();

        let win_data = WinData {
            id: node.id,
            scratch: scratchpad_ids.contains(&node.id),
        };
        windows.push_front(win_data);
    }).ok();

    thread::spawn(|| cmd_server(windowsc));

    // Listens to i3 event
    let events = conn.subscribe(&[EventType::Window]).unwrap();

    for event in events {
        if let Event::Window(e) = event.unwrap() {
            match e.change {
                WindowChange::Move => {
                    if e.container.nodes.len() > 0 {
                        scratchpad_ids.insert(e.container.nodes[0].id);
                    }
                },
                WindowChange::Focus => {
                    let mut windows = windows.lock().unwrap();
                    let cid = e.container.id;

                    // dedupe, push front and truncate
                    windows.retain(|v| v.id != cid);

                    let win_data = WinData {
                        id: cid,
                        scratch: scratchpad_ids.contains(&cid),
                    };
                    windows.push_front(win_data);

                    windows.truncate(BUFFER_SIZE);
                },
                WindowChange::Close => {
                    let mut windows = windows.lock().unwrap();
                    let cid = e.container.id;

                    // remove
                    windows.retain(|v| v.id != cid);
                },
                _ => {}
            }
        }
    }
}

fn focus_client(opts: SwitchOpts) {
    let mut stream = UnixStream::connect(socket_filename()).unwrap();

    // Just send a command to the server
    serde_json::to_vec(&Cmd::SwitchTo(opts))
        .map(move |b| stream.write_all(b.as_slice()))
        .ok();
}

fn extract_windows(root: &Node) -> HashMap<i64, &Node> {
    let mut out = HashMap::new();

    let mut expl = VecDeque::new();
    expl.push_front(root);
    while let Some(e) = expl.pop_front() {
        if e.node_type == NodeType::Con && e.nodes.is_empty() && e.floating_nodes.is_empty() {
            out.insert(e.id, e);
            continue;
        }

        if !e.marks.is_empty() {
            out.insert(e.id, e);
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

fn window_format_line(node: &Node, icons_map: &HashMap<String, String>) -> String {
    let mut marks = node.marks.join("][");
    if !node.marks.is_empty() {
        marks = format!(" [{}]", marks);
    }

    let mut ctype = "Container".to_string();
    let mut plus = "".to_string();
    if let Some(aid) = &node.app_id {
        ctype = aid.to_string();
    } else if let Some(props) = &node.window_properties {
        if let Some(c) = &props.class {
            ctype = c.to_string();
        }
    }
    if let Some(icon) = icons_map.get(&ctype) {
        if !icon.is_empty() {
            plus = format!("\0icon\x1f{}", icon);
        }
    }

    let mut name = "".to_string();
    if let Some(n) = &node.name {
        name = " - ".to_string() + n;
    }

    format!("{}{}<span weight=\"bold\">{}</span>{}\n", html_escape(&ctype), html_escape(&marks), html_escape(&name), plus)
}

fn choose_with_menu(menu: &str, icons_map: &HashMap<String, String>, windows: &[&Node]) -> Option<usize> {
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
            let line = window_format_line(&w, icons_map);
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

fn read_icons_map(icons_map: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();

    for (c, i) in DEFAULT_ICONS {
        m.insert((*c).to_string(), (*i).to_string());
    }

    let r = || -> Result<(), Box<dyn Error>> {
        let icons_map = icons_map.replace("~", &env::var("HOME")?);

        let f = fs::File::open(icons_map)?;
        let mn : HashMap<String, String> = serde_json::from_reader(f)?;

        for (k, v) in mn {
            m.insert(k, v);
        }
        Ok(())
    }();

    if let Err(e) = r {
        println!("Could not read icons map: {}", e);
    }

    m
}

fn focus_menu(menu_opts: MenuOpts) {
    let icons_map = read_icons_map(&menu_opts.icons_map);

    let mut conn = Connection::new().unwrap();

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

    if let Some(choice) = choose_with_menu(&menu_opts.menu, &icons_map, &ordered_windows) {
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

#[derive(Debug, Options, Serialize, Deserialize)]
struct SwitchOpts {
    #[options(help = "nth window to focus", no_long, short = "n", default = "1")]
    count: usize,
    #[options(help = "Don't count scratchpad", long = "ignore-scratchpad")]
    ignore_scratchpad: bool,
    #[options(help = "If scratchpad focused, hide it", long = "hide-scratchpad")]
    hide_scratchpad: bool,
}

#[derive(Debug, Options)]
struct ServerOpts {}

#[derive(Debug, Options)]
struct MenuOpts {
    #[options(help = "menu to run", default = "rofi -show-icons -dmenu -matching fuzzy -markup-rows -i -p window -format i")]
    menu: String,

    #[options(help = "path to icons map", default = "~/.config/i3-focus-last/icons.json")]
    icons_map: String,
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
        Some(ProgCommand::Server(s)) => { focus_server(s); }
        Some(ProgCommand::Switch(o)) => { focus_client(o); }
        Some(ProgCommand::Menu(m)) => { focus_menu(m); }
        _ => {
            let opts = SwitchOpts {
                count: 1,
                // remain compatible with previous behavior
                ignore_scratchpad: false,
                hide_scratchpad: false,
            };
            focus_client(opts);
        }
    }
}
