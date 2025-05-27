//! Monomorphized DLBasicString template using the provided encoding discriminator
//! used by the Dantelion2 FromSoftware in-house library.
//!
//! [`DLString`] instances can be read and written from existing structures, but not created.
//!
//! [`DLHashString`] instances cache its DLHash 32-bit hash using interior mutability.
//!
//! Thanks to Axi! for finding out the possible encoding tags.

use std::{
    borrow::Cow,
    ffi::OsString,
    fmt,
    mem::ManuallyDrop,
    os::windows::ffi::OsStringExt,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use cxx_stl::string::{CxxNarrowString, CxxUtf8String, CxxUtf16String, CxxUtf32String};

use crate::{hash::DLHash, stdalloc::DLStdAllocator};

/// Monomorphized `DLTX::DLBasicString` template using the provided encoding discriminator.
/// 
/// It can be read and written from existing structures, but not created.
#[repr(C)]
pub struct DLString {
    union: DLStringUnion,
    tag: DLStringTag,
}

/// Monomorphized `DLTX::DLBasicHashString` template based on [`DLString`].
/// 
/// Caches its DLHash 32-bit hash using interior mutability.
#[repr(C)]
#[derive(Debug)]
pub struct DLHashString {
    _vtable: usize,
    string: DLString,
    hash: DLStringHash,
}

#[repr(u8)]
#[allow(non_camel_case_types, dead_code)]
#[derive(Clone, Copy, Debug)]
enum DLStringTag {
    UTF8 = 0,
    UTF16 = 1,
    ISO_8859 = 2,
    SJIS = 3,
    EUC_JP = 4,
    UTF32 = 5,
}

#[repr(C)]
union DLStringUnion {
    utf8: ManuallyDrop<CxxUtf8String<DLStdAllocator>>,
    utf16: ManuallyDrop<CxxUtf16String<DLStdAllocator>>,
    iso_8859: ManuallyDrop<CxxNarrowString<DLStdAllocator>>,
    shift_jis: ManuallyDrop<CxxNarrowString<DLStdAllocator>>,
    euc_jp: ManuallyDrop<CxxNarrowString<DLStdAllocator>>,
    utf32: ManuallyDrop<CxxUtf32String<DLStdAllocator>>,
}

#[repr(C)]
#[derive(Debug)]
struct DLStringHash {
    value: AtomicU32,
    has_value: AtomicBool,
}

impl DLString {
    /// Reads the string and returns a UTF-8 encoded copy or a reference
    /// to the original string containing valid UTF-8 data.
    /// 
    /// Returns [`Some`] if the string contained data valid for its encoding, otherwise it returns [`None`].
    /// 
    /// Uses [encoding-rs](https://docs.rs/encoding_rs/0.8.35/encoding_rs/index.html) for decoding.
    pub fn read<'a>(&'a self) -> Option<Cow<'a, str>> {
        unsafe {
            match self.tag {
                DLStringTag::UTF8 => {
                    let (result, _, is_err) = encoding_rs::UTF_8.decode(self.union.utf8.as_bytes());
                    (!is_err).then_some(result)
                }
                DLStringTag::UTF16 => {
                    let result = OsString::from_wide(self.union.utf16.as_bytes());
                    Some(Cow::Owned(result.to_str()?.to_owned()))
                }
                DLStringTag::ISO_8859 => {
                    let (result, _, is_err) =
                        encoding_rs::ISO_8859_15.decode(self.union.iso_8859.as_bytes());
                    (!is_err).then_some(result)
                }
                DLStringTag::SJIS => {
                    let (result, _, is_err) =
                        encoding_rs::SHIFT_JIS.decode(self.union.shift_jis.as_bytes());
                    (!is_err).then_some(result)
                }
                DLStringTag::EUC_JP => {
                    let (result, _, is_err) =
                        encoding_rs::EUC_JP.decode(self.union.euc_jp.as_bytes());
                    (!is_err).then_some(result)
                }
                DLStringTag::UTF32 => {
                    let mut result = String::new();
                    for ch in self.union.utf32.as_bytes().iter() {
                        result.push(char::from_u32(*ch)?);
                    }
                    Some(Cow::Owned(result))
                }
            }
        }
    }

    /// Encodes the provided UTF-8 string with the source encoding and replaces
    /// the contents of `self` with `s`.
    /// 
    /// Returns `true` if the string could be encoded, otherwise it returns `false`
    /// and has no effect.
    /// 
    /// Uses [encoding-rs](https://crates.io/crates/encoding_rs) for encoding.
    pub fn write<T: AsRef<str>>(&mut self, s: T) -> bool {
        unsafe {
            match self.tag {
                DLStringTag::UTF8 => {
                    let dst = &mut *self.union.utf8;
                    let (result, _, is_err) = encoding_rs::UTF_8.encode(s.as_ref());
                    if !is_err {
                        *dst = CxxUtf8String::from_bytes_in(result, dst.allocator().clone());
                    }
                    !is_err
                }
                DLStringTag::UTF16 => {
                    let dst = &mut *self.union.utf16;
                    *dst = CxxUtf16String::new_in(dst.allocator().clone());
                    dst.extend(s.as_ref().encode_utf16());
                    true
                }
                DLStringTag::ISO_8859 => {
                    let dst = &mut *self.union.iso_8859;
                    let (result, _, is_err) = encoding_rs::ISO_8859_15.encode(s.as_ref());
                    if !is_err {
                        *dst = CxxNarrowString::from_bytes_in(result, dst.allocator().clone());
                    }
                    !is_err
                }
                DLStringTag::SJIS => {
                    let dst = &mut *self.union.shift_jis;
                    let (result, _, is_err) = encoding_rs::SHIFT_JIS.encode(s.as_ref());
                    if !is_err {
                        *dst = CxxNarrowString::from_bytes_in(result, dst.allocator().clone());
                    }
                    !is_err
                }
                DLStringTag::EUC_JP => {
                    let dst = &mut *self.union.euc_jp;
                    let (result, _, is_err) = encoding_rs::EUC_JP.encode(s.as_ref());
                    if !is_err {
                        *dst = CxxNarrowString::from_bytes_in(result, dst.allocator().clone());
                    }
                    !is_err
                }
                DLStringTag::UTF32 => {
                    let dst = &mut *self.union.utf32;
                    *dst = CxxUtf32String::new_in(dst.allocator().clone());
                    dst.extend(s.as_ref().chars().map(|c| c as u32));
                    true
                }
            }
        }
    }
}

impl DLHashString {
    pub fn read(&self) -> Option<Cow<'_, str>> {
        self.string.read()
    }

    pub fn write<T: AsRef<str>>(&mut self, s: T) {
        self.string.write(s);
        self.hash.has_value.store(false, Ordering::Relaxed);
    }
}

impl fmt::Debug for DLString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.read(), f)
    }
}

impl Drop for DLString {
    fn drop(&mut self) {
        unsafe {
            match self.tag {
                DLStringTag::UTF8 => ManuallyDrop::drop(&mut self.union.utf8),
                DLStringTag::UTF16 => ManuallyDrop::drop(&mut self.union.utf16),
                DLStringTag::ISO_8859 => ManuallyDrop::drop(&mut self.union.iso_8859),
                DLStringTag::SJIS => ManuallyDrop::drop(&mut self.union.shift_jis),
                DLStringTag::EUC_JP => ManuallyDrop::drop(&mut self.union.euc_jp),
                DLStringTag::UTF32 => ManuallyDrop::drop(&mut self.union.utf32),
            }
        }
    }
}

impl Clone for DLString {
    fn clone(&self) -> Self {
        Self {
            union: unsafe {
                match self.tag {
                    DLStringTag::UTF8 => DLStringUnion {
                        utf8: self.union.utf8.clone(),
                    },
                    DLStringTag::UTF16 => DLStringUnion {
                        utf16: self.union.utf16.clone(),
                    },
                    DLStringTag::ISO_8859 => DLStringUnion {
                        iso_8859: self.union.iso_8859.clone(),
                    },
                    DLStringTag::SJIS => DLStringUnion {
                        shift_jis: self.union.shift_jis.clone(),
                    },
                    DLStringTag::EUC_JP => DLStringUnion {
                        euc_jp: self.union.euc_jp.clone(),
                    },
                    DLStringTag::UTF32 => DLStringUnion {
                        utf32: self.union.utf32.clone(),
                    },
                }
            },
            tag: self.tag,
        }
    }
}

impl DLHash for DLString {
    fn strhash(&self) -> u32 {
        unsafe {
            match self.tag {
                DLStringTag::UTF8 => self.union.utf8.as_bytes().strhash(),
                DLStringTag::UTF16 => self.union.utf16.as_bytes().strhash(),
                DLStringTag::ISO_8859 => self.union.iso_8859.as_bytes().strhash(),
                DLStringTag::SJIS => self.union.shift_jis.as_bytes().strhash(),
                DLStringTag::EUC_JP => self.union.euc_jp.as_bytes().strhash(),
                DLStringTag::UTF32 => self.union.utf32.as_bytes().strhash(),
            }
        }
    }
}

impl Clone for DLHashString {
    fn clone(&self) -> Self {
        Self {
            _vtable: self._vtable,
            string: self.string.clone(),
            hash: unsafe { std::ptr::read(&self.hash) },
        }
    }
}

impl DLHash for DLHashString {
    fn strhash(&self) -> u32 {
        if self.hash.has_value.fetch_or(true, Ordering::AcqRel) {
            let hash = self.string.strhash();
            self.hash.value.store(hash, Ordering::Relaxed);
        }

        self.hash.value.load(Ordering::Relaxed)
    }
}

unsafe impl Send for DLHashString {}

unsafe impl Sync for DLHashString {}
