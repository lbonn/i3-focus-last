#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(improper_ctypes)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::ffi::CString;

#[macro_use]
extern crate byte_strings;

// TODO: find a way with const_concat_bytes!?
// static name_key : [u8; 128] = const_concat_bytes!(b"display-windowi3");
static name_key: [::std::os::raw::c_char; 128] = [
    0x64, 0x69, 0x73, 0x70, 0x6c, 0x61, 0x79, 0x2d, 0x77, 0x69, 0x6e, 0x64, 0x6f, 0x77, 0x69, 0x33,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

pub unsafe extern "C" fn _init(m: *mut Mode) -> ::std::os::raw::c_int {
    // this is freed by rofi
    (*m).display_name = CString::new("window").unwrap().into_raw();

    1
}

pub unsafe extern "C" fn _destroy(_m: *mut Mode) {}

pub unsafe extern "C" fn _get_num_entries(_m: *const Mode) -> ::std::os::raw::c_uint {
    0
}

pub unsafe extern "C" fn _result(
    _m: *mut Mode,
    menu_retv: ::std::os::raw::c_int,
    input: *mut *mut ::std::os::raw::c_char,
    selected_line: ::std::os::raw::c_uint,
) -> ModeMode {
    ModeMode_MODE_EXIT
}

pub unsafe extern "C" fn _token_match(
    _m: *const Mode,
    tokens: *mut *mut rofi_int_matcher,
    selected_line: ::std::os::raw::c_uint,
) -> ::std::os::raw::c_int {
    0
}

pub unsafe extern "C" fn _get_display_value(
    _m: *const Mode,
    selected_line: ::std::os::raw::c_uint,
    state: *mut ::std::os::raw::c_int,
    attribute_list: *mut *mut GList,
    get_entry: ::std::os::raw::c_int,
) -> *mut ::std::os::raw::c_char {
    std::ptr::null_mut()
}

pub unsafe extern "C" fn _preprocess_input(
    _m: *mut Mode,
    input: *const ::std::os::raw::c_char,
) -> *mut ::std::os::raw::c_char {
    std::ptr::null_mut()
}

pub unsafe extern "C" fn _get_message(_m: *const Mode) -> *mut ::std::os::raw::c_char {
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
