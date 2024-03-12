use bitflags::bitflags;
use std::alloc::{dealloc, Layout};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::ptr;
use std::sync::Mutex;

mod c {
    #![allow(non_camel_case_types)]
    #![allow(non_upper_case_globals)]
    #![allow(non_snake_case)]
    #![allow(improper_ctypes)]
    #![allow(dead_code)]
    #![allow(unknown_lints)]
    #![allow(clippy::all)]
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
    Exit = c::ModeMode_MODE_EXIT,
    NextDialog = c::ModeMode_NEXT_DIALOG,
    ReloadDialog = c::ModeMode_RELOAD_DIALOG,
    PreviousDialog = c::ModeMode_PREVIOUS_DIALOG,
    ResetDialog = c::ModeMode_RESET_DIALOG,
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

    pub fn find_arg_bool(name: &str) -> bool {
        unsafe {
            c::find_arg(name.as_ptr() as *const i8) != 0
        }
    }

    pub fn find_arg_i32(name: &str) -> Option<i32> {
        unsafe {
            let mut v : i32 = 0;
            if c::find_arg_int(name.as_ptr() as *const i8, &mut v) != 0 {
                return Some(v);
            }
            None
        }
    }

    pub fn find_arg_str(name: &str) -> Option<String> {
        unsafe {
            let mut v : *mut c_char = ptr::null_mut();
            if c::find_arg_str(name.as_ptr() as *const i8, &mut v) != 0 {
                return Some(CStr::from_ptr(v).to_str().unwrap().to_string());
            }
            None
        }
    }

    pub fn token_match_pattern(pattern: &Pattern, token: &str) -> bool {
        unsafe {
            // :)
            let mself: *mut Pattern = &mut (std::mem::transmute(*pattern));
            let mut ftokens: [*mut c::rofi_int_matcher; 2] = [mself, ptr::null_mut()];
            c::helper_token_match(ftokens.as_mut_ptr(), token.as_ptr() as *const i8) != 0
        }
    }

    pub fn token_match_patterns(patterns: &Vec<&Pattern>, token: &str) -> bool {
        let mut ftokens: Vec<*mut Pattern> = vec![];
        unsafe {
            for p in patterns {
                ftokens.push(&mut (std::mem::transmute(**p)));
            }
            ftokens.push(ptr::null_mut());

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
    fn result(&self, mretv: MenuReturn, selected_line: usize) -> Option<ModeMode>;
    fn token_match(&self, patterns: Vec<&Pattern>, selected_line: usize) -> bool;
    fn icon_query(&self, selected_line: usize) -> Option<String>;
}

#[derive(Debug, Eq, PartialEq, Hash)]
struct IconCacheEntry {
    line: usize,
    height: usize,
    scale: usize,
}

type IconCache = HashMap<IconCacheEntry, c_uint>;

struct ModeData<T: RofiMode> {
    mode: T,
    icon_cache: Mutex<IconCache>,
}

impl<T: RofiMode> ModeData<T> {
    fn init() -> Result<Self, ()> {
        let mode = T::init()?;
        let icon_cache = Mutex::new(HashMap::new());
        Ok(ModeData { mode, icon_cache })
    }
}

impl c::rofi_mode {
    fn get<T: RofiMode>(&self) -> &ModeData<T> {
        unsafe { &*(self.private_data as *const ModeData<T>) }
    }
}

unsafe extern "C" fn _init<T: RofiMode>(mc: *mut c::rofi_mode) -> c_int {
    (*mc).display_name = T::DISPLAY_NAME.to_owned().into_raw();

    match ModeData::<T>::init() {
        Ok(d) => {
            (*mc).private_data = Box::into_raw(Box::new(d)) as *mut c_void;
            1
        }
        Err(_) => 0,
    }
}

unsafe extern "C" fn _destroy<T: RofiMode>(mc: *mut c::rofi_mode) {
    if (*mc).private_data.is_null() {
        return;
    }

    ptr::drop_in_place((*mc).private_data as *mut ModeData<T>);
    dealloc((*mc).private_data as *mut u8, Layout::new::<ModeData<T>>());
    (*mc).private_data = ptr::null_mut();
}

unsafe extern "C" fn _get_num_entries<T: RofiMode>(mc: *const c::rofi_mode) -> c_uint {
    let m = (*mc).get::<T>();
    m.mode.get_num_entries().try_into().unwrap()
}

unsafe extern "C" fn _get_display_value<T: RofiMode>(
    mc: *const c::rofi_mode,
    selected_line: c_uint,
    state: *mut c_int,
    _attribute_list: *mut *mut c::GList,
    get_entry: c_int,
) -> *mut c_char {
    let m = (*mc).get::<T>();

    if let Some((dv, flags)) = m.mode.get_display_value(selected_line as usize) {
        *state = flags.bits() as i32;

        if get_entry == 0 {
            return ptr::null_mut();
        }

        CString::new(dv.as_bytes()).unwrap().into_raw()
    } else {
        ptr::null_mut()
    }
}

unsafe extern "C" fn _result<T: RofiMode>(
    mc: *mut c::rofi_mode,
    mretv: c_int,
    _input: *mut *mut c_char,
    selected_line: c_uint,
) -> c::ModeMode {
    let m = (*mc).get::<T>();

    // TODO: pass input

    match m.mode.result(
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
    while !(*t).is_null() {
        tokenv.push(&**t);
        t = t.add(1);
    }

    let m = (*mc).get::<T>();
    m.mode.token_match(tokenv, selected_line as usize) as c_int
}

unsafe extern "C" fn _get_icon<T: RofiMode>(
    mc: *const c::rofi_mode,
    selected_line: c_uint,
    height: c_uint,
) -> *mut c::cairo_surface_t {
    let m = (*mc).get::<T>();

    let entry = IconCacheEntry {
        line: selected_line as usize,
        height: height as usize,
        scale: 1, // TODO: handle this "cleanly"
    };

    // it's not a problem to keep this lock open for a while
    // as _get_icon calls (like all the mode api) are never
    // called in parallel
    let mut icon_cache = m.icon_cache.lock().unwrap();

    let mut icon_uid = None;
    if let Some(uid) = icon_cache.get(&entry) {
        icon_uid = Some(*uid)
    } else if let Some(mut query) = m.mode.icon_query(selected_line as usize) {
        let uid = c::rofi_icon_fetcher_query(
            query.as_mut_ptr() as *const i8,
            height as ::std::os::raw::c_int,
        );

        icon_cache.insert(entry, uid);
        icon_uid = Some(uid);
    }

    icon_uid
        .map(|u| c::rofi_icon_fetcher_get(u))
        .unwrap_or(ptr::null_mut())
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
        mc._get_icon = Some(_get_icon::<T>);
        mc.type_ = T::TYPE as u32;

        mc
    }
}
