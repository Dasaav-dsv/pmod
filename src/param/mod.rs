//! Param row manipulation utilities.
//!
//! - Retrieve with [`ParamRepository::get_row`]
//! - Insert with [`ParamRepository::insert_row`]
//! - Replace with [`ParamRepository::replace_row`]
//! - Delete with [`ParamRepository::delete_row`]
//! 
//! Param row manipulation uses a free list approach with
//! amortized O(1) insertion and removal performance.
//! 
//! Original implementation idea by tremwil.

use std::{borrow::Cow, error, fmt, ptr::NonNull, sync::LazyLock};

use file::FileHeader;
use windows::core::w;

use crate::{
    hash::DLHash,
    image_base,
    resource::{ResCap, ResCapHolderItem, ResRep},
    static_lock::{StaticLock, StaticPtr},
    stdalloc::DLStdAllocator,
};

pub mod file;

pub use file::Error as FileError;

/// Static `FD4Singleton` holding `FD4ParamResCap`s.
#[repr(C)]
pub struct ParamRepository {
    res_rep: ResRep<ParamResCap>,
    alloc: DLStdAllocator,
}

/// A `FD4ParamResCap`, "resource capsule" holding a param file.
#[repr(C)]
pub struct ParamResCap {
    res_cap: ResCap<Self>,
    file_size: usize,
    file: NonNull<FileHeader>,
}

/// Possible param manipulation errors.
#[derive(Clone, Copy, Debug)]
pub enum Error {
    /// Mismatch in input or file format.
    FormatError(FileError),

    /// Static [`ParamRepository`] instance is null.
    NullInstance,

    /// The param table with the specified `DLHash` does not exist.
    TableNotFound,
}

/// Param manipulation result.
pub type Result<T> = std::result::Result<T, Error>;

static PARAM_REPOSITORY: LazyLock<StaticLock<ParamRepository>> =
    LazyLock::new(|| StaticLock::new());

impl ParamRepository {
    /// Finds a param table [`ParamResCap`] by its `DLHash`.
    ///
    /// # Errors:
    /// - [`Error::TableNotFound`]
    pub fn find_table<'a, T: DLHash>(&'a self, s: T) -> Result<&'a ParamResCap> {
        self.raw_find_table(s).map(|t| unsafe { t.as_ref() })
    }

    /// Finds a param table [`ParamResCap`] by its `DLHash`.
    ///
    /// # Errors:
    /// - [`Error::TableNotFound`]
    pub fn find_table_mut<'a, T: DLHash>(&'a mut self, s: T) -> Result<&'a mut ParamResCap> {
        self.raw_find_table(s).map(|mut t| unsafe { t.as_mut() })
    }

    /// Searches for a row by its id with a binary search, returning a pointer to its data.
    ///
    /// `id` must be a non-negative signed 32-bit integer.
    ///
    /// # Errors:
    /// - [`FileError::NegativeId`] if `id` is negative.
    /// - [`FileError::NotInTable`] if the corresponding row is not found.
    /// - [`FileError::Malformed`] if param file can't be parsed.
    /// - [`Error::NullInstance`] if static [`ParamRepository`] instance is null.
    /// - [`Error::TableNotFound`]
    pub fn get_row<T: DLHash>(s: T, id: i32) -> Result<NonNull<u8>> {
        let repo = PARAM_REPOSITORY.read().ok_or(Error::NullInstance)?;

        let table = repo.find_table(s)?;
        let file = table.file();

        Ok(file.find_row(id)?)
    }

    /// Tries to insert a new row with fields pointed to by `data`
    /// and returns its positive id.
    ///
    /// `data` must be valid for the lifetime of the param file.
    ///
    /// # Errors:
    /// - [`FileError::FailedRealloc`] if necessary file reallocation failed.
    /// - [`FileError::Malformed`] if popping from the free list returned an invalid entry.
    /// - [`Error::NullInstance`] if static [`ParamRepository`] instance is null.
    /// - [`Error::TableNotFound`]
    pub fn insert_row<T: DLHash>(s: T, data: NonNull<u8>) -> Result<i32> {
        let mut repo = PARAM_REPOSITORY.write().ok_or(Error::NullInstance)?;

        let table = repo.find_table_mut(s)?;
        let file = table.file_mut();

        if let Ok(new_id) = file.insert_row(data) {
            return Ok(new_id);
        }

        let (new_file, new_size) = file.clone_reallocate(true)?;

        table.file = NonNull::from(&mut *new_file);
        table.file_size = new_size;

        Ok(new_file.insert_row(data)?)
    }

    /// Searches for a row by its id with a binary search and replaces its fields,
    /// returning a pointer to its old field data.
    ///
    /// `id` must be a non-negative signed 32-bit integer.
    ///
    /// # Errors:
    /// - [`FileError::NegativeId`] if `id` is negative.
    /// - [`FileError::NotInTable`] if the corresponding row is not found.
    /// - [`FileError::FailedRealloc`] if necessary file reallocation failed.
    /// - [`FileError::Malformed`] if param file can't be parsed.
    /// - [`Error::NullInstance`] if static [`ParamRepository`] instance is null.
    /// - [`Error::TableNotFound`]
    pub fn replace_row<T: DLHash>(s: T, id: i32, data: NonNull<u8>) -> Result<NonNull<u8>> {
        let mut repo = PARAM_REPOSITORY.write().ok_or(Error::NullInstance)?;

        let table = repo.find_table_mut(s)?;
        let file = table.file_mut();

        if let Ok(new_id) = file.replace_row(id, data) {
            return Ok(new_id);
        }

        let (new_file, new_size) = file.clone_reallocate(false)?;

        table.file = NonNull::from(&mut *new_file);
        table.file_size = new_size;

        Ok(new_file.replace_row(id, data)?)
    }

    /// Searches for a row by its id with a binary search and deletes it,
    /// returning a pointer to its old field data.
    ///
    /// `id` must be a non-negative signed 32-bit integer.
    ///
    /// # Errors:
    /// - [`FileError::NegativeId`] if `id` is negative.
    /// - [`FileError::FailedRealloc`] if necessary file reallocation failed.
    /// - [`FileError::Malformed`] if pushing to the free list returned an invalid entry.
    /// - [`Error::NullInstance`] if static [`ParamRepository`] instance is null.
    /// - [`Error::TableNotFound`]
    pub fn delete_row<T: DLHash>(s: T, id: i32) -> Result<NonNull<u8>> {
        let mut repo = PARAM_REPOSITORY.write().ok_or(Error::NullInstance)?;

        let table = repo.find_table_mut(s)?;
        let file = table.file_mut();

        if let Ok(data) = file.delete_row(id) {
            return Ok(data);
        }

        let (new_file, new_size) = file.clone_reallocate(true)?;

        table.file = NonNull::from(&mut *new_file);
        table.file_size = new_size;

        Ok(new_file.delete_row(id)?)
    }

    fn raw_find_table<'a, T: DLHash>(&'a self, s: T) -> Result<NonNull<ParamResCap>> {
        unsafe {
            let hash = s.strhash();

            let mut bucket = self.res_rep.holder.bucket_for_hash(hash);

            while let Some(next) = bucket {
                let next = next.as_ref();
                bucket = next.res_cap.item.next;

                if next.res_cap.item.name.strhash() == hash {
                    return Ok(next.into());
                }
            }
        }

        Err(Error::TableNotFound)
    }
}

impl ParamResCap {
    /// Get the held file by its header.
    pub fn file(&self) -> &FileHeader {
        unsafe { self.file.as_ref() }
    }

    /// Get the held file by its header.
    pub fn file_mut(&mut self) -> &mut FileHeader {
        unsafe { self.file.as_mut() }
    }

    /// Get the size of file in bytes.
    pub fn file_size(&self) -> usize {
        self.file_size
    }
}

impl AsRef<ResCapHolderItem<ParamResCap>> for ParamResCap {
    fn as_ref(&self) -> &ResCapHolderItem<ParamResCap> {
        self.res_cap.as_ref()
    }
}

impl fmt::Debug for ParamResCap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self
            .res_cap
            .item
            .name
            .read()
            .unwrap_or(Cow::Borrowed("ERROR"));

        f.debug_struct("ParamResCap")
            .field("name", &name.as_ref())
            .field("file", &self.file)
            .finish()
    }
}

impl fmt::Debug for ParamRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_list();

        unsafe {
            let buckets = self.res_rep.holder.as_slice();

            for mut bucket in buckets.iter().copied() {
                while let Some(next) = bucket {
                    let next = next.as_ref();
                    bucket = next.res_cap.item.next;

                    dbg.entry(next);
                }
            }
        }

        dbg.finish_non_exhaustive()
    }
}

impl StaticPtr for ParamRepository {
    const STATIC_ID: windows::core::PCWSTR = w!("PMOD_PARAM_REPOSITORY");

    fn static_ptr() -> NonNull<*mut Self> {
        unsafe { image_base().byte_add(0x485dea0).cast() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self, f)
    }
}

impl error::Error for Error {}

impl From<FileError> for Error {
    fn from(value: FileError) -> Self {
        Self::FormatError(value)
    }
}

unsafe impl Send for ParamResCap {}

unsafe impl Sync for ParamResCap {}
