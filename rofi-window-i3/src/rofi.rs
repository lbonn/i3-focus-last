use bitflags::bitflags;
use std::alloc::{dealloc, Layout};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint, c_void};

mod c {
    #![allow(non_camel_case_types)]
    #![allow(non_upper_case_globals)]
    #![allow(non_snake_case)]
    #![allow(improper_ctypes)]
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use c::rofi_int_matcher_t as Pattern;
pub use c::rofi_mode as CRofiMode;

#[repr(u32)]
#[derive(Debug)]
pub enum ModeType {
    Unset = c::ModeType_MODE_TYPE_UNSET,
    Switcher = c::ModeType_MODE_TYPE_SWITCHER,
    Completer = c::ModeType_MODE_TYPE_COMPLETER,
    Dmenu = c::ModeType_MODE_TYPE_DMENU,
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct MenuReturn: u32 {
        const Ok = c::MenuReturn_MENU_OK;
        const Cancel = c::MenuReturn_MENU_CANCEL;
        const Next = c::MenuReturn_MENU_NEXT;
        const CustomInput = c::MenuReturn_MENU_CUSTOM_INPUT;
        const EntryDelete = c::MenuReturn_MENU_ENTRY_DELETE;
        const QuickSwitch = c::MenuReturn_MENU_QUICK_SWITCH;
        const CustomCommand = c::MenuReturn_MENU_CUSTOM_COMMAND;
        const Previous = c::MenuReturn_MENU_PREVIOUS;
        const Complete = c::MenuReturn_MENU_COMPLETE;
        const CustomAction = c::MenuReturn_MENU_CUSTOM_ACTION;
    }
}

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub struct EntryStateFlags: u32 {
      const Normal = 0;
      const Urgent = 1;
      const Active = 2;
      const Selected = 4;
      const Markup = 8;
      const Alt = 16;
      const Highlight = 32;
      const FmodMask = 48;  // state + highlight
    }
}

#[repr(u32)]
#[derive(Debug)]
pub enum ModeMode {
    Exit = c::ModeMode_MODE_EXIT as u32,
    NextDialog = c::ModeMode_NEXT_DIALOG as u32,
    ReloadDialog = c::ModeMode_RELOAD_DIALOG as u32,
    PreviousDialog = c::ModeMode_PREVIOUS_DIALOG as u32,
    ResetDialog = c::ModeMode_RESET_DIALOG as u32,
}

#[macro_export]
macro_rules! rofi_name_key {
    (
        $single:expr $(,)?
    ) => {
        unsafe {
            &*std::mem::transmute::<_, &[c_char; 128]>(const_concat_bytes!(
                $single,
                &[0u8; 128 - $single.len()]
            ))
        }
    };
}

/// API that can be called from rust modes
pub mod helpers {
    use crate::rofi::c;
    use crate::rofi::*;

    pub fn token_match_pattern(pattern: &Pattern, token: &str) -> bool {
        unsafe {
            // :)
            let mself: *mut Pattern = &mut (std::mem::transmute(*pattern));
            let mut ftokens: [*mut c::rofi_int_matcher; 2] = [mself, std::ptr::null_mut()];
            c::helper_token_match(ftokens.as_mut_ptr(), token.as_ptr() as *const i8) != 0
        }
    }

    pub fn token_match_patterns(patterns: &Vec<&Pattern>, token: &str) -> bool {
        let mut ftokens: Vec<*mut Pattern> = vec![];
        unsafe {
            for p in patterns {
                ftokens.push(&mut (std::mem::transmute(**p)));
            }
            ftokens.push(std::ptr::null_mut());

            c::helper_token_match(ftokens.as_mut_ptr(), token.as_ptr() as *const i8) != 0
        }
    }

    pub fn rofi_view_hide() {
        // this is internal API, subject to break!
        unsafe {
            c::rofi_view_hide();
        }
    }
}

pub trait RofiMode: Sized {
    const NAME: &'static CStr;
    const DISPLAY_NAME: &'static CStr;
    const NAME_KEY: &'static [c_char; 128];
    const TYPE: ModeType;

    fn init() -> Result<Self, ()>;
    fn get_num_entries(&self) -> usize;
    // TODO: pango attributes
    fn get_display_value(&self, selected_line: usize) -> Option<(String, EntryStateFlags)>;
    fn result(&mut self, mretv: MenuReturn, selected_line: usize) -> Option<ModeMode>;
    fn token_match(&self, patterns: Vec<&Pattern>, selected_line: usize) -> bool;
}

impl c::rofi_mode {
    fn get<T: RofiMode>(&self) -> &T {
        unsafe { &*(self.private_data as *const T) }
    }

    fn get_mut<T: RofiMode>(&mut self) -> &mut T {
        unsafe { &mut *(self.private_data as *mut T) }
    }
}

unsafe extern "C" fn _init<T: RofiMode>(mc: *mut c::rofi_mode) -> c_int {
    (*mc).display_name = T::DISPLAY_NAME.to_owned().into_raw();

    (|| -> Result<(), ()> {
        let d = Box::new(T::init()?);
        (*mc).private_data = Box::into_raw(d) as *mut c_void;

        Ok(())
    })()
    .map_or_else(|_| 0, |_| 1)
}

unsafe extern "C" fn _destroy<T: RofiMode>(mc: *mut c::rofi_mode) {
    std::ptr::drop_in_place((*mc).private_data as *mut T);
    dealloc((*mc).private_data as *mut u8, Layout::new::<T>());
    (*mc).private_data = std::ptr::null_mut();
}

unsafe extern "C" fn _get_num_entries<T: RofiMode>(mc: *const c::rofi_mode) -> c_uint {
    let m = (*mc).get::<T>();
    m.get_num_entries().try_into().unwrap()
}

unsafe extern "C" fn _get_display_value<T: RofiMode>(
    mc: *const c::rofi_mode,
    selected_line: c_uint,
    state: *mut c_int,
    _attribute_list: *mut *mut c::GList,
    get_entry: c_int,
) -> *mut c_char {
    let m = (*mc).get::<T>();

    if let Some((dv, flags)) = m.get_display_value(selected_line as usize) {
        *state = flags.bits() as i32;

        if get_entry == 0 {
            return std::ptr::null_mut();
        }

        CString::new(dv.as_bytes()).unwrap().into_raw()
    } else {
        std::ptr::null_mut()
    }
}

unsafe extern "C" fn _result<T: RofiMode>(
    mc: *mut c::rofi_mode,
    mretv: c_int,
    _input: *mut *mut c_char,
    selected_line: c_uint,
) -> c::ModeMode {
    let m = (*mc).get_mut::<T>();

    // TODO: pass input

    match m.result(
        MenuReturn::from_bits(mretv as u32).unwrap(),
        selected_line.try_into().unwrap(),
    ) {
        Some(e) => e as c_uint,
        None => (mretv as u32) & c::MenuReturn_MENU_LOWER_MASK,
    }
}

unsafe extern "C" fn _token_match<T: RofiMode>(
    mc: *const c::rofi_mode,
    tokens: *mut *mut c::rofi_int_matcher,
    selected_line: c_uint,
) -> c_int {
    let mut tokenv: Vec<&Pattern> = vec![];
    let mut t = tokens;
    while *t != std::ptr::null_mut() {
        tokenv.push(&**t);
        t = t.add(1);
    }

    let m = (*mc).get::<T>();
    m.token_match(tokenv, selected_line as usize) as c_int
}

pub const fn rofi_c_mode<T: RofiMode>() -> c::rofi_mode {
    unsafe {
        let mut mc: c::rofi_mode = std::mem::zeroed();
        mc.abi_version = c::ABI_VERSION;
        mc.name = T::NAME.as_ptr() as *mut i8;
        mc.cfg_name_key = *T::NAME_KEY;

        mc._init = Some(_init::<T>);
        mc._destroy = Some(_destroy::<T>);
        mc._get_num_entries = Some(_get_num_entries::<T>);
        mc._get_display_value = Some(_get_display_value::<T>);
        mc._result = Some(_result::<T>);
        mc._token_match = Some(_token_match::<T>);
        mc.type_ = T::TYPE as u32;

        mc
    }
}
