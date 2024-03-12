pub mod rofi;

use std::collections::HashMap;
use std::sync::Mutex;

use std::ffi::CStr;
use std::os::raw::c_char;

use i3_focus_last::get_windows_by_history;
use i3_focus_last::utils;

use rofi::helpers::{rofi_view_hide, token_match_patterns};
use rofi::{CRofiMode, EntryStateFlags, MenuReturn, ModeMode, ModeType, Pattern, RofiMode};

#[macro_use]
extern crate byte_strings;

struct Mode {
    pub conn: Mutex<swayipc::Connection>,
    pub windows: Vec<swayipc::Node>,
    pub icons_map: HashMap<String, String>,
}

impl RofiMode for Mode {
    const NAME: &'static CStr = c_str!("window-i3");
    const DISPLAY_NAME: &'static CStr = c_str!("window");
    const TYPE: ModeType = ModeType::Switcher;
    const NAME_KEY: &'static [c_char; 128] = rofi_name_key!(b"display-windowi3");

    fn init() -> Result<Self, ()> {
        let mut conn = swayipc::Connection::new().map_err(|_| ())?;
        let windows = get_windows_by_history(&mut conn).map_err(|_| ())?;
        let icons_map = utils::read_icons_map(None);

        Ok(Mode {
            conn: Mutex::new(conn),
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
                .lock()
                .unwrap()
                .run_command(format!("[con_id={}] focus", win.id).as_str())
                .unwrap();
        }

        Some(ModeMode::Exit)
    }

    fn token_match(&self, patterns: Vec<&Pattern>, selected_line: usize) -> bool {
        assert!(selected_line < self.windows.len());

        let win = &self.windows[selected_line];

        // TODO check other fields (appid) if requested

        if let Some(name) = win.name.as_ref() {
            if !token_match_patterns(&patterns, name) {
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

#[no_mangle]
pub static mut mode: CRofiMode = rofi::rofi_c_mode::<Mode>();
