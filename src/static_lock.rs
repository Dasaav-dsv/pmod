use std::{
    mem,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use windows::{
    Win32::{
        Foundation::{GetLastError, INVALID_HANDLE_VALUE},
        System::{
            Memory::{CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, PAGE_READWRITE},
            Threading::{
                AcquireSRWLockExclusive, AcquireSRWLockShared, ReleaseSRWLockExclusive,
                ReleaseSRWLockShared, SRWLOCK,
            },
        },
    },
    core::PCWSTR,
};

pub struct StaticLock<T: ?Sized + StaticPtr> {
    lock: NonNull<SRWLOCK>,
    ptr: NonNull<*mut T>,
}

pub struct StaticLockReadGuard<'a, T: ?Sized> {
    value: &'a T,
    lock: NonNull<SRWLOCK>,
}

pub struct StaticLockWriteGuard<'a, T: ?Sized> {
    value: &'a mut T,
    lock: NonNull<SRWLOCK>,
}

impl<T: ?Sized + StaticPtr> StaticLock<T> {
    pub fn new() -> Self {
        const RWLOCK_SIZE: usize = mem::size_of::<SRWLOCK>();

        let ptr = T::static_ptr();

        unsafe {
            // Starts zero-initialized, valid for SRWLOCK.
            let mapping_handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                RWLOCK_SIZE as u32,
                T::STATIC_ID,
            )
            .expect("CreateFileMappingW failed");

            let mapping =
                MapViewOfFile(mapping_handle, FILE_MAP_ALL_ACCESS, 0, 0, RWLOCK_SIZE).Value;

            let Some(lock) = NonNull::new(mapping as _) else {
                panic!("MapViewOfFile failed: {}", GetLastError().ok().unwrap_err());
            };

            Self { lock, ptr }
        }
    }

    pub fn read(&self) -> Option<StaticLockReadGuard<'_, T>> {
        let ptr = NonNull::new(unsafe { self.ptr.read() })?;
        Some(StaticLockReadGuard::new(self.lock, ptr))
    }

    pub fn write(&self) -> Option<StaticLockWriteGuard<'_, T>> {
        let ptr = NonNull::new(unsafe { self.ptr.read() })?;
        Some(StaticLockWriteGuard::new(self.lock, ptr))
    }
}

impl<T: ?Sized> StaticLockReadGuard<'_, T> {
    fn new(lock: NonNull<SRWLOCK>, ptr: NonNull<T>) -> Self {
        unsafe {
            AcquireSRWLockShared(lock.as_ptr());

            Self {
                value: ptr.as_ref(),
                lock,
            }
        }
    }
}

impl<T: ?Sized> StaticLockWriteGuard<'_, T> {
    fn new(lock: NonNull<SRWLOCK>, mut ptr: NonNull<T>) -> Self {
        unsafe {
            AcquireSRWLockExclusive(lock.as_ptr());

            Self {
                value: ptr.as_mut(),
                lock,
            }
        }
    }
}

pub trait StaticPtr {
    const STATIC_ID: PCWSTR;

    fn static_ptr() -> NonNull<*mut Self>;
}

impl<T: ?Sized> Deref for StaticLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T: ?Sized> Deref for StaticLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T: ?Sized> DerefMut for StaticLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

impl<T: ?Sized> Drop for StaticLockReadGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ReleaseSRWLockShared(self.lock.as_ptr());
        }
    }
}

impl<T: ?Sized> Drop for StaticLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ReleaseSRWLockExclusive(self.lock.as_ptr());
        }
    }
}

unsafe impl<T: ?Sized + Send + StaticPtr> Send for StaticLock<T> {}

unsafe impl<T: ?Sized + Send + Sync + StaticPtr> Sync for StaticLock<T> {}

unsafe impl<T: ?Sized + Send> Send for StaticLockReadGuard<'_, T> {}

unsafe impl<T: ?Sized + Send + Sync> Sync for StaticLockReadGuard<'_, T> {}

unsafe impl<T: ?Sized + Send> Send for StaticLockWriteGuard<'_, T> {}

unsafe impl<T: ?Sized + Send + Sync> Sync for StaticLockWriteGuard<'_, T> {}
