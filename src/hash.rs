//! Hash used by FromSoftware for resource names and file paths.

use std::ffi::CStr;

/// 32-bit non-cryptographic case insensitive hash
/// used by FromSoftware for resource names and file paths.
pub trait DLHash {
    /// Get the representation's hash.
    ///
    /// If two strings in lower case compare equal, their hashes must be equal.
    fn strhash(&self) -> u32;
}

impl DLHash for &str {
    fn strhash(&self) -> u32 {
        dl_hash(self.as_bytes().iter().copied())
    }
}

impl DLHash for &CStr {
    fn strhash(&self) -> u32 {
        dl_hash(self.to_bytes().iter().copied())
    }
}

impl DLHash for &[u8] {
    fn strhash(&self) -> u32 {
        dl_hash(self.iter().copied())
    }
}

impl DLHash for &[u16] {
    fn strhash(&self) -> u32 {
        dl_hash(self.iter().copied())
    }
}

impl DLHash for &[u32] {
    fn strhash(&self) -> u32 {
        dl_hash(self.iter().copied())
    }
}

fn dl_hash<I>(i: I) -> u32
where
    I: IntoIterator<Item: Into<u32>>,
{
    let mut result = 0u32;

    for ch in i.into_iter() {
        let mut ch = ch.into();

        if ch <= 'Z' as u32 {
            // To lowercase
            ch += 32;
        } else if ch == '\\' as u32 {
            // Treat backslashes as slashes
            ch = '/' as u32;
        }

        result = result.wrapping_mul(137);
        result = result.wrapping_add(ch);
    }

    result
}
