pub mod rofi;

use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;

use std::ffi::CStr;
use std::os::raw::c_char;

use i3_focus_last::utils;
use i3_focus_last::{WindowsSortStyle, get_windows_by_history};

use rofi::helpers::{find_arg_bool, rofi_view_hide, token_match_pattern};
use rofi::{CRofiMode, EntryStateFlags, MenuReturn, ModeMode, ModeType, Pattern, RofiMode};

#[macro_use]
extern crate byte_strings;

struct Mode {
    conn: RefCell<swayipc::Connection>,
    windows: Vec<swayipc::Node>,
    icons_map: HashMap<String, String>,
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

        Ok(Mode {
            conn: RefCell::new(conn),
            windows,
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
            self.conn
                .borrow_mut()
                .run_command(format!("[con_id={}] focus", win.id).as_str())
                .unwrap();
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
}

#[unsafe(no_mangle)]
pub static mut mode: CRofiMode = rofi::rofi_c_mode::<Mode>();
