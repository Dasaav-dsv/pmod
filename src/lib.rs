#![doc = include_str!("../README.md")]

use std::{
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use windows::{
    Win32::System::LibraryLoader::GetModuleHandleW,
    core::PCWSTR,
};

pub mod exports;

pub mod fmg;
pub mod hash;
pub mod param;
mod resource;
mod static_lock;
pub mod stdalloc;
mod string;

fn image_base() -> NonNull<u8> {
    static IMAGE_BASE: AtomicUsize = AtomicUsize::new(0);

    let mut image_base = IMAGE_BASE.load(Ordering::Acquire);
    if image_base == 0 {
        image_base = unsafe {
            GetModuleHandleW(PCWSTR::null())
                .expect("GetModuleHandleW failed")
                .0 as usize
        };

        IMAGE_BASE.store(image_base, Ordering::Release);
    }

    NonNull::new(image_base as _).expect("GetModuleHandleW failed")
}
