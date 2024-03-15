use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;

pub mod ipc;

use crate::ipc::client::get_focus_history;

fn extract_windows(root: &swayipc::Node) -> HashMap<i64, &swayipc::Node> {
    let mut out = HashMap::new();

    let mut expl = VecDeque::new();
    expl.push_front(root);
    while let Some(e) = expl.pop_front() {
        if e.node_type == swayipc::NodeType::Con
            && e.nodes.is_empty()
            && e.floating_nodes.is_empty()
        {
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

pub mod utils {
    use std::collections::HashMap;
    use std::env;
    use std::error::Error;
    use std::fs;

    pub type IconsMap = HashMap<String, String>;

    static DEFAULT_ICONS: &[(&str, &str)] = &[("Chromium", "chromium")];

    pub fn read_icons_map(icons_map: Option<&str>) -> IconsMap {
        let icons_map = icons_map.unwrap_or("~/.config/i3-focus-last/icons.json");
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

    /// Returns the app_id or class of a node
    pub fn node_display_id(node: &swayipc::Node) -> Option<String> {
        if let Some(aid) = &node.app_id {
            return Some(aid.to_string());
        } else if let Some(props) = &node.window_properties {
            if let Some(c) = &props.class {
                return Some(c.to_string());
            }
        }

        None
    }

    /// Returns the icon name for a given node, including conversion
    /// with supplied icons map
    pub fn node_icon_name(
        node: &swayipc::Node,
        icons_map: &HashMap<String, String>,
    ) -> Option<String> {
        if let Some(disp_id) = node_display_id(node).as_ref() {
            if let Some(icon) = icons_map.get(disp_id) {
                if !icon.is_empty() {
                    return Some(icon.clone());
                }
            }
            return Some(disp_id.clone());
        }

        None
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

    pub fn window_format_line(
        node: &swayipc::Node,
        icons_map: Option<&HashMap<String, String>>,
    ) -> String {
        let mut marks = node.marks.join("][");
        if !node.marks.is_empty() {
            marks = format!(" [{}]", marks);
        }

        let disp_id = node_display_id(node);

        let mut plus = "".to_string();
        if let Some(icons_map) = icons_map.as_ref() {
            if let Some(icon_name) = node_icon_name(node, icons_map) {
                plus = format!("\0icon\x1f{}", icon_name);
            }
        }

        let mut name = "".to_string();
        if let Some(n) = &node.name {
            name = " - ".to_string() + n;
        }

        format!(
            "{}{}<span weight=\"bold\">{}</span>{}\n",
            html_escape(&disp_id.unwrap_or("Container".to_string())),
            html_escape(&marks),
            html_escape(&name),
            plus
        )
    }

    pub fn get_focused_window(
        conn: &mut swayipc::Connection,
    ) -> Result<i64, Box<dyn Error + Send + Sync>> {
        let mut node = conn.get_tree()?;

        while !node.focused {
            let fid = node.focus.into_iter().next().ok_or("")?;
            node = node.nodes.into_iter().find(|n| n.id == fid).ok_or("")?;
        }
        Ok(node.id)
    }
}

#[derive(PartialEq, Eq)]
pub enum WindowsSortStyle {
    CurrentLast,
    CurrentFirst,
}

/// Returns the list of current windows in most-recently-used order
///
/// It will try to connect to the i3-focus-last server if available and will
/// default to the order returned by the WM otherwise.
pub fn get_windows_by_history(
    conn: &mut swayipc::Connection,
    sort_style: WindowsSortStyle,
) -> Result<Vec<swayipc::Node>, Box<dyn Error + Send + Sync>> {
    let t = conn.get_tree()?;
    let ws = extract_windows(&t);

    let mut hist = get_focus_history().unwrap_or_else(|e| {
        eprintln!(
            "warning: could not get focus history: \"{}\", order will be arbitrary",
            e
        );
        vec![]
    });

    let mut ordered_windows: Vec<swayipc::Node> = vec![];
    let mut removed = HashSet::new();
    if sort_style == WindowsSortStyle::CurrentLast && !hist.is_empty() {
        hist.remove(0);
    }
    for i in hist {
        if let Some(n) = ws.get(&i) {
            ordered_windows.push((*n).clone());
            removed.insert(i);
        }
    }
    for (i, w) in ws {
        if !removed.contains(&i) {
            ordered_windows.push(w.clone());
        }
    }

    Ok(ordered_windows)
}

// re-exports
pub use crate::ipc::client::focus_nth_last_client;
pub use crate::ipc::server::{focus_server, ServerOpts};
