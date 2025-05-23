//! FD4 resource abstractions.
//!
//! Credits to vswarte and eldenring-rs for some of the layouts

use std::{ptr::NonNull, slice};

use crate::{stdalloc::DLStdAllocator, string::DLHashString};

#[repr(C)]
pub struct ResCap<T>
where
    T: AsRef<ResCapHolderItem<T>>,
{
    pub item: ResCapHolderItem<T>,
    #[cfg(feature = "elden-ring")]
    is_debug: bool,
    #[cfg(any(feature = "elden-ring", feature = "sekiro"))]
    _unk61: bool,
    #[cfg(feature = "elden-ring")]
    debug_item: usize,
    #[cfg(feature = "elden-ring")]
    _unk70: bool,
}

#[repr(C)]
pub struct ResRep<T>
where
    T: AsRef<ResCapHolderItem<T>>,
{
    pub resource: ResCap<T>,
    pub holder: ResCapHolder<T>,
}

#[repr(C)]
pub struct ResCapHolder<T>
where
    T: AsRef<ResCapHolderItem<T>>,
{
    _vtable: usize,
    pub alloc: DLStdAllocator,
    pub owner: Option<NonNull<ResRep<T>>>,
    _unk18: u32,
    pub len: u32,
    pub buckets: NonNull<Option<NonNull<T>>>,
}

#[repr(C)]
pub struct ResCapHolderItem<T>
where
    T: AsRef<ResCapHolderItem<T>>,
{
    _vtable: usize,
    pub name: DLHashString,
    pub owner: Option<NonNull<ResCapHolder<T>>>,
    pub next: Option<NonNull<T>>,
    pub refcount: u32,
}

impl<T> ResCapHolder<T>
where
    T: AsRef<ResCapHolderItem<T>>,
{
    pub unsafe fn bucket_for_hash(&self, hash: u32) -> Option<NonNull<T>> {
        let index = hash % self.len;
        unsafe { self.buckets.add(index as usize).read() }
    }

    pub unsafe fn as_slice(&self) -> &[Option<NonNull<T>>] {
        unsafe { slice::from_raw_parts(self.buckets.as_ptr(), self.len as usize) }
    }

    pub unsafe fn as_mut_slice(&mut self) -> &mut [Option<NonNull<T>>] {
        unsafe { slice::from_raw_parts_mut(self.buckets.as_ptr(), self.len as usize) }
    }
}

impl<T> AsRef<ResCapHolderItem<T>> for ResCap<T>
where
    T: AsRef<ResCapHolderItem<T>>,
{
    fn as_ref(&self) -> &ResCapHolderItem<T> {
        &self.item
    }
}

impl<T> AsRef<ResCapHolderItem<T>> for ResRep<T>
where
    T: AsRef<ResCapHolderItem<T>>,
{
    fn as_ref(&self) -> &ResCapHolderItem<T> {
        &self.resource.item
    }
}

unsafe impl<T: AsRef<ResCapHolderItem<T>>> Send for ResCap<T> {}

unsafe impl<T: AsRef<ResCapHolderItem<T>>> Sync for ResCap<T> {}

unsafe impl<T: AsRef<ResCapHolderItem<T>>> Send for ResRep<T> {}

unsafe impl<T: AsRef<ResCapHolderItem<T>>> Sync for ResRep<T> {}
