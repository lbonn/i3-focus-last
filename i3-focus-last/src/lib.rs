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
            if let Some(disp_id) = disp_id.as_ref() {
                if let Some(icon) = icons_map.get(disp_id) {
                    if !icon.is_empty() {
                        plus = format!("\0icon\x1f{}", icon);
                    }
                } else {
                    plus = format!("\0icon\x1f{}", disp_id);
                }
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
}

/// Returns the list of current windows in most-recently-used order
///
/// It will try to connect to the i3-focus-last server if available and will
/// default to the order returned by the WM otherwise.
pub fn get_windows_by_history(
    conn: &mut swayipc::Connection,
) -> Result<Vec<swayipc::Node>, Box<dyn Error + Send + Sync>> {
    let t = conn.get_tree()?;
    let ws = extract_windows(&t);

    let mut hist = get_focus_history().unwrap_or_default();

    let mut ordered_windows: Vec<swayipc::Node> = vec![];
    let mut removed = HashSet::new();
    if !hist.is_empty() {
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
