//! `pmod.dll` function exports.
//!
//! For C bindings look at "include/pmod.h"

use std::{
    ffi::{c_char, CStr},
    num::NonZeroU32,
    ptr::NonNull,
};

use crate::{fmg::MsgRepository, param::ParamRepository};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_get_row(table_name: *const c_char, id: i32) -> Option<NonNull<u8>> {
    if table_name.is_null() || id < 0 {
        return None;
    }

    let table_name = unsafe { CStr::from_ptr(table_name) };

    ParamRepository::get_row(table_name, id).ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_insert_row(table_name: *const c_char, data: *mut u8) -> i32 {
    if table_name.is_null() {
        return -1;
    }

    let Some(data) = NonNull::new(data) else {
        return -1;
    };

    let table_name = unsafe { CStr::from_ptr(table_name) };

    ParamRepository::insert_row(table_name, data).unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_replace_row(
    table_name: *const c_char,
    id: i32,
    data: *mut u8,
) -> Option<NonNull<u8>> {
    if table_name.is_null() || id < 0 {
        return None;
    }

    let data = NonNull::new(data)?;

    let table_name = unsafe { CStr::from_ptr(table_name) };

    ParamRepository::replace_row(table_name, id, data).ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_delete_row(
    table_name: *const c_char,
    id: i32,
) -> Option<NonNull<u8>> {
    if table_name.is_null() || id < 0 {
        return None;
    }

    let table_name = unsafe { CStr::from_ptr(table_name) };

    ParamRepository::delete_row(table_name, id).ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_get_msg(
    version: u32,
    category: u32,
    id: u32,
) -> Option<NonNull<u16>> {
    MsgRepository::get_msg(version, category, id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_insert_msg(
    version: u32,
    category: u32,
    data: *mut u16,
) -> Option<NonZeroU32> {
    MsgRepository::insert_msg(version, category, None, NonNull::new(data))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_replace_msg(
    version: u32,
    category: u32,
    id: u32,
    data: *mut u16,
) -> Option<NonNull<u16>> {
    MsgRepository::replace_msg(version, category, id, NonNull::new(data))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pmod_delete_msg(
    version: u32,
    category: u32,
    id: u32,
) -> Option<NonNull<u16>> {
    MsgRepository::replace_msg(version, category, id, None)
}
