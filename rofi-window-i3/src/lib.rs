pub mod rofi;

use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::sync::mpsc;

use std::ffi::CStr;
use std::os::raw::c_char;
use std::thread;
use std::time;

use i3_focus_last::utils;
use i3_focus_last::{
    WindowsSortStyle, get_windows_by_history, push_to_history, release_inhibit_history,
    take_inhibit_history,
};

use rofi::helpers::{find_arg_bool, rofi_view_hide, token_match_pattern};
use rofi::{CRofiMode, EntryStateFlags, MenuReturn, ModeMode, ModeType, Pattern, RofiMode};

#[macro_use]
extern crate byte_strings;

struct HistInhibitor {
    token: u64,
    stop_send: mpsc::Sender<()>,
    handle: Option<thread::JoinHandle<()>>,
}

impl HistInhibitor {
    fn new() -> Result<Self, Box<dyn Error>> {
        let (stop_send, recv) = mpsc::channel();
        let token = take_inhibit_history(None)?;
        let handle = thread::spawn(move || {
            loop {
                if let Some(()) = recv.recv_timeout(time::Duration::from_secs(1)).ok() {
                    break;
                }

                take_inhibit_history(Some(token)).unwrap();
            }
        });

        Ok(Self {
            token,
            handle: Some(handle),
            stop_send,
        })
    }
}

impl Drop for HistInhibitor {
    fn drop(&mut self) {
        println!("bye");
        if !self.handle.is_none() {
            self.stop_send.send(()).ok();
            self.handle.take().map(thread::JoinHandle::join);
        }
        release_inhibit_history(self.token).map_err(|_| ()).ok();
    }
}

struct HistoryPreview {
    #[allow(dead_code)]
    pub hist_inhibitor: HistInhibitor,
    pub final_focus: RefCell<Option<i64>>,
    pub current_focus: RefCell<Option<i64>>,
}

struct Mode {
    pub conn: RefCell<swayipc::Connection>,
    pub windows: Vec<swayipc::Node>,
    pub history_preview: Option<HistoryPreview>,
    pub icons_map: HashMap<String, String>,
}

impl Drop for Mode {
    fn drop(&mut self) {
        if let Some(hp) = &self.history_preview {
            hp.final_focus.borrow_mut().map(|id| {
                push_to_history(id).unwrap();
                self.focus(id)
            });
        }
    }
}

impl Mode {
    fn focus_if_needed(&self, id: i64) {
        if let Some(hp) = &self.history_preview {
            if *hp.current_focus.borrow() == Some(id) {
                return;
            }
        }

        self.focus(id);
    }

    fn focus(&self, id: i64) {
        self.conn
            .borrow_mut()
            .run_command(format!("[con_id={}] focus", id).as_str())
            .unwrap();

        if let Some(hp) = &self.history_preview {
            *hp.current_focus.borrow_mut() = Some(id);
        }
    }
}

impl RofiMode for Mode {
    const NAME: &'static CStr = c_str!("window-i3");
    const DISPLAY_NAME: &'static CStr = c_str!("window");
    const TYPE: ModeType = ModeType::Switcher;
    const NAME_KEY: &'static [c_char; 128] = rofi_name_key!(b"display-windowi3");

    fn init() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let sort_style = if find_arg_bool("-window-focused-first") {
            WindowsSortStyle::CurrentFirst
        } else {
            WindowsSortStyle::CurrentLast
        };

        let mut conn = swayipc::Connection::new()?;
        let windows = get_windows_by_history(&mut conn, sort_style)?;
        let icons_map = utils::read_icons_map(None);

        let preview_history = find_arg_bool("-window-preview-focus");

        let history_preview = if preview_history {
            let hist_inhibitor = HistInhibitor::new().unwrap();
            let init_focus = utils::get_focused_window(&conn.get_tree()?).ok();
            Some(HistoryPreview {
                hist_inhibitor,
                final_focus: RefCell::new(init_focus),
                current_focus: RefCell::new(init_focus),
            })
        } else {
            None
        };

        Ok(Mode {
            conn: RefCell::new(conn),
            windows,
            history_preview,
            icons_map,
        })
    }

    fn get_num_entries(&self) -> usize {
        self.windows.len()
    }

    fn get_display_value(&self, selected_line: usize) -> Option<(String, EntryStateFlags)> {
        assert!(selected_line < self.windows.len());

        let win = &self.windows[selected_line];

        Some((
            utils::window_format_line(win, None),
            EntryStateFlags::Markup,
        ))
    }

    fn result(&self, mretv: MenuReturn, selected_line: usize) -> Option<ModeMode> {
        if mretv.intersects(MenuReturn::CustomAction) {
            return None;
        } else if mretv.intersects(MenuReturn::Ok) {
            assert!(selected_line < self.windows.len());

            rofi_view_hide();

            let win = &self.windows[selected_line];

            if let Some(hp) = &self.history_preview {
                // will be done in `destroy`
                *hp.final_focus.borrow_mut() = Some(win.id);
            } else {
                self.focus_if_needed(win.id);
            }
        }

        Some(ModeMode::Exit)
    }

    fn token_match(&self, patterns: &[Pattern], selected_line: usize) -> bool {
        assert!(selected_line < self.windows.len());

        let win = &self.windows[selected_line];

        for pat in patterns {
            let mut m = false;

            if let Some(name) = win.name.as_ref() {
                m = token_match_pattern(pat, name);
            }
            if m == (pat.invert != 0)
                && let Some(appid) = win.app_id.as_ref()
            {
                m = token_match_pattern(pat, appid);
            }

            if !m {
                return false;
            }
        }

        true
    }

    fn icon_query(&self, selected_line: usize) -> Option<String> {
        assert!(selected_line < self.windows.len());
        let win = &self.windows[selected_line];
        utils::node_icon_name(win, &self.icons_map)
    }

    fn selection_changed(&self, selected_line: Option<usize>) -> bool {
        if let Some(hp) = &self.history_preview {
            let focus_to = match selected_line {
                None => *hp.final_focus.borrow(),
                Some(l) => {
                    assert!(l < self.windows.len());
                    let win = &self.windows[l];
                    Some(win.id)
                }
            };

            if let Some(id) = focus_to {
                self.focus_if_needed(id);
                return true;
            }
        }

        false
    }
}

#[unsafe(no_mangle)]
pub static mut mode: CRofiMode = rofi::rofi_c_mode::<Mode>();
