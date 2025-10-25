#![doc = include_str!("../README.md")]

#[cfg(feature = "exports")]
pub mod exports;

pub mod fmg;
pub mod hash;
pub mod param;
mod resource;
mod static_lock;
pub mod stdalloc;
mod string;
