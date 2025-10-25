//! `fmg` format string manipulation utilities.
//!
//! - Retrieve with [`MsgRepository::get_msg`]
//! - Insert with [`MsgRepository::insert_msg`]
//! - Replace with [`MsgRepository::replace_msg`]

use std::{fmt, num::NonZeroU32, ptr::NonNull, slice, sync::LazyLock};

use file::FileHeader;
use from_singleton::FromSingleton;
use windows::core::w;

use crate::{
    static_lock::{StaticLock, StaticPtr},
    stdalloc::DLStdAllocator,
};

mod file;

#[repr(C)]
pub struct MsgRepository {
    inner: FD4MessageManager,
    _unk38: u32,
    _unk3c: u32,
    _unk40: u32,
    _unk44: u32,
}

#[repr(C)]
struct FD4MessageManager {
    _vtable: usize,
    inner: NonNull<Option<NonNull<Option<NonNull<FileHeader>>>>>,
    version_count: u32,
    file_capacity: u32,
    _unk18: u32,
    _unk20: usize,
    _unk28: usize,
    alloc: DLStdAllocator,
}

static MSG_REPOSITORY: LazyLock<StaticLock<MsgRepository>> = LazyLock::new(|| StaticLock::new());

impl MsgRepository {
    pub fn get_msg(version: u32, category: u32, id: u32) -> Option<NonNull<u16>> {
        let repo = MSG_REPOSITORY.read()?;
        let file = repo.file_by_category(version, category)?;

        let index = file.msg_index_by_id(id)?;
        
        file.msg_data_by_index(index)
    }

    pub fn insert_msg(version: u32, category: u32, after: Option<NonZeroU32>, data: Option<NonNull<u16>>) -> Option<NonZeroU32> {
        let mut repo = MSG_REPOSITORY.write()?;

        let after = after.or_else(|| repo.new_after(category))?;
        let file = repo.file_by_category_mut(version, category)?;

        let old_file = unsafe { file.as_mut() };

        if let new_id @ Some(_) = old_file.try_insert_new_after(after, data) {
            return new_id;
        }

        let new_file = old_file.grow_reallocate(after)?;
        *file = new_file.into();

        new_file.try_insert_new_after(after, data)
    }

    pub fn replace_msg(version: u32, category: u32, id: u32, data: Option<NonNull<u16>>) -> Option<NonNull<u16>> {
        let mut repo = MSG_REPOSITORY.write()?;
        let file = unsafe { repo.file_by_category_mut(version, category)?.as_mut() };

        let index = file.msg_index_by_id(id)?;

        file.replace_msg_by_index(index, data)
    }

    pub fn get_all_msgs(version: u32, category: u32) -> Option<Vec<(u32, NonNull<u16>)>> {
        let repo = MSG_REPOSITORY.read()?;
        let file = repo.file_by_category(version, category)?;
        Some(file.all_msgs().collect())
    }

    pub fn get_all_categories(version: u32) -> Option<Vec<u32>> {
        let repo = MSG_REPOSITORY.read()?;
        let holder = repo.inner.by_version(version)?;
        Some(holder.files().iter().enumerate().filter_map(|(i, o)| o.and(Some(i as u32))).collect())
    }

    fn file_by_category(&self, version: u32, category: u32) -> Option<&FileHeader> {
        let holder = self.inner.by_version(version)?;
        let ptr = *holder.files().get(category as usize)?;
        Some(unsafe { ptr?.as_ref() })
    }

    fn file_by_category_mut<'a>(&'a mut self, version: u32, category: u32) -> Option<&'a mut NonNull<FileHeader>> {
        let mut holder = self.inner.by_version(version)?;
        let ptr = holder.files_mut().get_mut(category as usize)?;
        ptr.as_mut()
    }

    fn new_after(&self, category: u32) -> Option<NonZeroU32> {
        const MAX_BASE: NonZeroU32 = NonZeroU32::new(999_999_999).unwrap();
        const MAX_DIFF: u32 = u32::MAX - MAX_BASE.get();

        if category >= self.inner.file_capacity {
            return None;
        }

        let step = MAX_DIFF.checked_div(self.inner.file_capacity)?;

        MAX_BASE.checked_add(step * category)
    }
}

impl FD4MessageManager {
    fn by_version(&self, v: u32) -> Option<FileHolder<'_>> {
        let versions =
            unsafe { slice::from_raw_parts_mut(self.inner.as_ptr(), self.version_count as _) };

        let version = versions.get_mut(v as usize)?.as_mut();

        Some(FileHolder {
            inner: version?,
            file_capacity: self.file_capacity,
        })
    }
}

impl<'a> FileHolder<'a> {
    fn files(&self) -> &'a [Option<NonNull<FileHeader>>] {
        unsafe { slice::from_raw_parts(self.inner.as_ptr(), self.file_capacity as _) }
    }

    fn files_mut(&mut self) -> &'a mut [Option<NonNull<FileHeader>>] {
        unsafe { slice::from_raw_parts_mut(self.inner.as_ptr(), self.file_capacity as _) }
    }
}

struct FileHolder<'a> {
    inner: &'a mut NonNull<Option<NonNull<FileHeader>>>,
    file_capacity: u32,
}

impl fmt::Debug for MsgRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MsgRepository")
            .field("version_count", &self.inner.version_count)
            .field("file_capacity", &self.inner.file_capacity)
            .field("alloc", &self.inner.alloc)
            .field("files", &self.inner)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for FD4MessageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries((0..self.version_count).filter_map(|v| self.by_version(v)))
            .finish()
    }
}

impl fmt::Debug for FileHolder<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(
                self.files()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, o)| o.and(Some(i))),
            )
            .finish()
    }
}

impl FromSingleton for MsgRepository {}

impl StaticPtr for MsgRepository {
    const STATIC_ID: windows::core::PCWSTR = w!("PMOD_MSG_REPOSITORY");
}

unsafe impl Send for MsgRepository {}

unsafe impl Sync for MsgRepository {}
