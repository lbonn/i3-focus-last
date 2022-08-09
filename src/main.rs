#[macro_use]
extern crate serde_derive;

extern crate gumdrop;
extern crate serde;
extern crate serde_json;
extern crate swayipc;

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::from_utf8;
use std::sync::{Arc, Mutex};
use std::thread;

use gumdrop::Options;
use swayipc::{Connection, EventType};
use swayipc::{Event, Node, NodeType, WindowChange};

use serde::Deserialize;

static BUFFER_SIZE: usize = 100;
static DEFAULT_ICONS: &[(&str, &str)] = &[("firefox", "firefox"), ("Chromium", "chromium")];

fn socket_filename() -> Result<String, Box<env::VarError>> {
    Ok(env::var("HOME")? + "/.local/share/i3-focus-last.sock")
}

/// Commands sent for client-server interfacing
#[derive(Serialize, Deserialize, Debug)]
enum Cmd {
    SwitchTo(usize),
    GetHistory,
}

fn focus_nth(windows: &VecDeque<i64>, n: usize) -> Result<(), Box<dyn Error>> {
    let mut conn = Connection::new()?;
    let mut k = n;

    // Start from the nth window and try to change focus until it succeeds
    // (so that it skips windows which no longer exist)
    while let Some(wid) = windows.get(k) {
        let r = conn.run_command(format!("[con_id={}] focus", wid).as_str())?;

        if let Some(o) = r.get(0) {
            if o.is_ok() {
                return Ok(());
            }
        }

        k += 1;
    }

    Err(From::from(format!("Last {}nth window unavailable", n)))
}

fn cmd_server(windows: Arc<Mutex<VecDeque<i64>>>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket = socket_filename()?;
    let socket = Path::new(&socket);

    if socket.exists() {
        fs::remove_file(&socket)?;
    }

    // Listen to client commands
    let listener = UnixListener::bind(socket)?;

    for stream in listener.incoming().flatten() {
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

    Ok(())
}

fn get_focused_window() -> Result<Result<i64, ()>, Box<dyn Error + Send + Sync>> {
    let mut conn = Connection::new()?;
    let mut node = conn.get_tree()?;

    Ok(|| -> Result<_, ()> {
        while !node.focused {
            let fid = node.focus.into_iter().next().ok_or(())?;
            node = node.nodes.into_iter().find(|n| n.id == fid).ok_or(())?;
        }
        Ok(node.id)
    }())
}

fn focus_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    let conn = Connection::new()?;
    let windows = Arc::new(Mutex::new(VecDeque::new()));
    let windowsc = Arc::clone(&windows);

    // Add the current focused window to bootstrap the list
    get_focused_window()?
        .map(|wid| {
            let mut windows = windows.lock().unwrap();

            windows.push_front(wid);
        })
        .ok();

    // Listens to i3 event
    let events = conn.subscribe(&[EventType::Window])?;

    let server_handle = thread::spawn(|| cmd_server(windowsc));

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

    server_handle.join().unwrap()?;

    Ok(())
}

fn focus_client(nth_window: usize) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    // Just send a command to the server
    serde_json::to_vec(&Cmd::SwitchTo(nth_window))
        .map(move |b| stream.write_all(b.as_slice()))
        .ok();

    Ok(())
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
            expl.push_front(c);
        }
        for c in &e.floating_nodes {
            expl.push_front(c);
        }
    }

    out
}

fn get_focus_history() -> Result<Vec<i64>, Box<dyn Error>> {
    let mut stream = UnixStream::connect(socket_filename()?)?;

    // Just send a command to the server
    let out =
        serde_json::to_vec(&Cmd::GetHistory).map(move |b| -> Result<_, Box<dyn Error>> {
            stream.write_all(b.as_slice())?;
            let mut de = serde_json::Deserializer::from_reader(&stream);
            let o = Vec::deserialize(&mut de)?;
            Ok(o)
        })??;
    Ok(out)
}

fn html_escape(instr: &str) -> String {
    instr
        .chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&#39;".chars().collect(),
            _ => vec![c],
        })
        .collect()
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

    format!(
        "{}{}<span weight=\"bold\">{}</span>{}\n",
        html_escape(&ctype),
        html_escape(&marks),
        html_escape(&name),
        plus
    )
}

fn choose_with_menu(
    menu: &str,
    icons_map: &HashMap<String, String>,
    windows: &[&Node],
) -> Result<usize, Box<dyn Error + Send + Sync>> {
    // TODO: better split
    let cmd: Vec<&str> = menu.split(' ').collect();

    let mut child = Command::new(cmd[0])
        .args(cmd[1..].iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "no stdin"))?;
        for w in windows {
            let line = window_format_line(w, icons_map);
            stdin.write_all(line.as_bytes())?;
        }
    }

    let out = child.wait_with_output()?;
    let s = from_utf8(out.stdout.as_slice())?;
    let s: String = s.chars().filter(|x| !matches!(x, ' ' | '\n')).collect();

    Ok(s.parse()?)
}

fn read_icons_map(icons_map: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();

    for (c, i) in DEFAULT_ICONS {
        m.insert((*c).to_string(), (*i).to_string());
    }

    let r = || -> Result<(), Box<dyn Error>> {
        let icons_map = icons_map.replace('~', &env::var("HOME")?);

        let f = fs::File::open(icons_map)?;
        let mn: HashMap<String, String> = serde_json::from_reader(f)?;

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

fn focus_menu(menu_opts: MenuOpts) -> Result<(), Box<dyn Error + Send + Sync>> {
    let icons_map = read_icons_map(&menu_opts.icons_map);

    let mut conn = Connection::new()?;

    let t = conn.get_tree()?;
    let ws = extract_windows(&t);

    let mut hist = get_focus_history().unwrap_or_default();
    let mut ordered_windows: Vec<&Node> = vec![];
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

    let choice = choose_with_menu(&menu_opts.menu, &icons_map, &ordered_windows)?;
    let wid = ordered_windows[choice].id;
    conn.run_command(format!("[con_id={}] focus", wid).as_str())?;

    Ok(())
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
    #[options(
        help = "menu to run",
        default = "rofi -show-icons -dmenu -matching fuzzy -markup-rows -i -p window -format i"
    )]
    menu: String,

    #[options(
        help = "path to icons map",
        default = "~/.config/i3-focus-last/icons.json"
    )]
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

fn main() -> Result<(), String> {
    let opts = ProgOptions::parse_args_default_or_exit();

    if opts.version {
        println!("i3-focus-last {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let r = match opts.command {
        Some(ProgCommand::Server(_)) => focus_server(),
        Some(ProgCommand::Switch(o)) => focus_client(o.count),
        Some(ProgCommand::Menu(m)) => focus_menu(m),
        _ => focus_client(1),
    };

    if let Err(ref e) = r {
        return Err(format!("{}", e));
    }

    Ok(())
}
