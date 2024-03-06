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
