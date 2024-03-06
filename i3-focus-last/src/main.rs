use gumdrop::Options;

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::io::Write;
use std::process::{Command, Stdio};
use std::str::from_utf8;

use i3_focus_last::{focus_nth_last_client, focus_server, get_windows_by_history, ServerOpts};

#[derive(Debug, Options)]
pub struct MenuOpts {
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
struct ProgOptions {
    #[options(help = "help")]
    help: bool,

    #[options(help = "version")]
    version: bool,

    #[options(command)]
    command: Option<ProgCommand>,
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

fn window_format_line(node: &swayipc::Node, icons_map: &HashMap<String, String>) -> String {
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
    windows: &[swayipc::Node],
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

static DEFAULT_ICONS: &[(&str, &str)] = &[("firefox", "firefox"), ("Chromium", "chromium")];

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

    let mut conn = swayipc::Connection::new()?;

    let ordered_windows = crate::get_windows_by_history(&mut conn)?;

    let choice = choose_with_menu(&menu_opts.menu, &icons_map, &ordered_windows)?;
    let wid = ordered_windows[choice].id;
    conn.run_command(format!("[con_id={}] focus", wid).as_str())?;

    Ok(())
}

fn main() -> Result<(), String> {
    let opts = ProgOptions::parse_args_default_or_exit();

    if opts.version {
        println!("i3-focus-last {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let r = match opts.command {
        Some(ProgCommand::Server(_)) => focus_server(),
        Some(ProgCommand::Switch(o)) => focus_nth_last_client(o.count),
        Some(ProgCommand::Menu(m)) => focus_menu(m),
        _ => focus_nth_last_client(1),
    };

    if let Err(ref e) = r {
        return Err(format!("{}", e));
    }

    Ok(())
}