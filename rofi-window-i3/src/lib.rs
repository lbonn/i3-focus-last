#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(improper_ctypes)]
#![allow(unused_variables)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::alloc::{dealloc, Layout};
use std::error::Error;
use std::ffi::CString;

use std::os::raw::{c_char, c_int, c_uint, c_void};

use swayipc;

use i3_focus_last::get_windows_by_history;

#[macro_use]
extern crate byte_strings;

// TODO: find a way with const_concat_bytes!?
// static name_key : [u8; 128] = const_concat_bytes!(b"display-windowi3");
static name_key: [c_char; 128] = [
    0x64, 0x69, 0x73, 0x70, 0x6c, 0x61, 0x79, 0x2d, 0x77, 0x69, 0x6e, 0x64, 0x6f, 0x77, 0x69, 0x33,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

struct ModeData {
    pub conn: Option<swayipc::Connection>,
    pub windows: Vec<swayipc::Node>,
}

impl ModeData {
    pub fn new() -> Self {
        ModeData {
            conn: None,
            windows: vec![],
        }
    }

    pub fn init_connection(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(self.conn = Some(swayipc::Connection::new()?))
    }

    pub fn fetch_windows_list(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let wins = get_windows_by_history(self.conn.as_mut().ok_or("err")?)?;
        self.windows = wins;
        Ok(())
    }
}

impl Mode {
    fn get_mode_data_mut(&mut self) -> &mut ModeData {
        if self.private_data.is_null() {
            let d: Box<ModeData> = Box::new(ModeData::new());
            self.private_data = Box::into_raw(d) as *mut c_void;
        }
        unsafe { &mut *(self.private_data as *mut ModeData) }
    }

    fn get_mode_data(&self) -> &ModeData {
        unsafe { &*(self.private_data as *const ModeData) }
    }

    fn destroy_mode_data(&mut self) {
        unsafe {
            std::ptr::drop_in_place(self.private_data as *mut ModeData);
            dealloc(self.private_data as *mut u8, Layout::new::<ModeData>());
            self.private_data = std::ptr::null_mut();
        }
    }
}

pub unsafe extern "C" fn _init(m: *mut Mode) -> c_int {
    // this is freed by rofi
    (*m).display_name = CString::new("window").unwrap().into_raw();

    let mode_data = (*m).get_mode_data_mut();

    (|| -> Result<(), Box<dyn Error + Send + Sync>> {
        mode_data.init_connection()?;

        mode_data.fetch_windows_list()?;

        Ok(())
    })()
    .map_or_else(|_| 0, |_| 1)
}

pub unsafe extern "C" fn _destroy(m: *mut Mode) {
    (*m).destroy_mode_data();
}

pub unsafe extern "C" fn _get_num_entries(m: *const Mode) -> c_uint {
    let mode_data = (*m).get_mode_data();

    mode_data.windows.len() as c_uint
}

pub unsafe extern "C" fn _result(
    m: *mut Mode,
    mretv: c_int,
    input: *mut *mut c_char,
    selected_line: c_uint,
) -> ModeMode {
    let mode_data = (*m).get_mode_data_mut();
    let mut retv = ModeMode_MODE_EXIT;
    let mretv = mretv as c_uint;

    if mretv & MenuReturn_MENU_NEXT != 0 {
        retv = ModeMode_NEXT_DIALOG;
    } else if mretv & MenuReturn_MENU_PREVIOUS != 0 {
        retv = ModeMode_PREVIOUS_DIALOG;
    } else if mretv & MenuReturn_MENU_QUICK_SWITCH != 0 {
        retv = mretv & MenuReturn_MENU_LOWER_MASK;
    } else if mretv & MenuReturn_MENU_OK != 0 {
        let win = &mode_data.windows[selected_line as usize];
        mode_data
            .conn
            .as_mut()
            .unwrap()
            .run_command(format!("[con_id={}] focus", win.id).as_str())
            .unwrap();
    }

    retv
}

pub unsafe extern "C" fn _token_match(
    _m: *const Mode,
    tokens: *mut *mut rofi_int_matcher,
    selected_line: c_uint,
) -> c_int {
    0
}

pub unsafe extern "C" fn _get_display_value(
    m: *const Mode,
    selected_line: c_uint,
    _state: *mut c_int,
    _attribute_list: *mut *mut GList,
    get_entry: c_int,
) -> *mut c_char {
    let mode_data = (*m).get_mode_data();

    if get_entry == 0 {
        return std::ptr::null_mut();
    }

    CString::new(match &mode_data.windows[selected_line as usize].name {
        Some(t) => t.as_bytes(),
        None => b"",
    })
    .unwrap()
    .into_raw()
}

pub unsafe extern "C" fn _preprocess_input(_m: *mut Mode, input: *const c_char) -> *mut c_char {
    std::ptr::null_mut()
}

pub unsafe extern "C" fn _get_message(_m: *const Mode) -> *mut c_char {
    std::ptr::null_mut()
}

#[no_mangle]
pub static mut mode: rofi_mode = rofi_mode {
    abi_version: ABI_VERSION,
    name: c_str!("window-i3").as_ptr() as *mut i8,
    cfg_name_key: name_key,
    display_name: std::ptr::null_mut(),
    _init: Some(_init),
    _destroy: Some(_destroy),
    _get_num_entries: Some(_get_num_entries),
    _result: Some(_result),
    _token_match: Some(_token_match),
    _get_display_value: Some(_get_display_value),
    _selection_changed: None,
    _get_icon: None,
    _get_completion: None,
    _preprocess_input: None,
    _get_message: Some(_get_message),
    private_data: std::ptr::null_mut(),
    free: None,
    _create: None,
    _completer_result: None,
    ed: std::ptr::null_mut(),
    module: std::ptr::null_mut(),
    fallback_icon_fetch_uid: 0,
    fallback_icon_not_found: 0,
    type_: ModeType_MODE_TYPE_SWITCHER,
};
