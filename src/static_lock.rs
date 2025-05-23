use std::{
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use from_singleton::FromSingleton;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GetLastError, INVALID_HANDLE_VALUE},
        System::{
            Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE},
            Threading::{
                AcquireSRWLockExclusive, AcquireSRWLockShared, ReleaseSRWLockExclusive,
                ReleaseSRWLockShared, SRWLOCK,
            },
        },
    },
};

pub struct StaticLock<T: StaticPtr> {
    lock: NonNull<SRWLOCK>,
    _marker: PhantomData<T>,
}

pub struct StaticLockReadGuard<'a, T> {
    value: &'a T,
    lock: NonNull<SRWLOCK>,
}

pub struct StaticLockWriteGuard<'a, T> {
    value: &'a mut T,
    lock: NonNull<SRWLOCK>,
}

impl<T: StaticPtr + FromSingleton> StaticLock<T> {
    pub fn new() -> Self {
        const RWLOCK_SIZE: usize = mem::size_of::<SRWLOCK>();

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

            Self {
                lock,
                _marker: PhantomData,
            }
        }
    }

    pub fn read(&self) -> Option<StaticLockReadGuard<'_, T>> {
        Some(StaticLockReadGuard::new(
            self.lock,
            from_singleton::address_of::<T>()?,
        ))
    }

    pub fn write(&self) -> Option<StaticLockWriteGuard<'_, T>> {
        Some(StaticLockWriteGuard::new(
            self.lock,
            from_singleton::address_of::<T>()?,
        ))
    }
}

impl<T> StaticLockReadGuard<'_, T> {
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

impl<T> StaticLockWriteGuard<'_, T> {
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
}

impl<T> Deref for StaticLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T> Deref for StaticLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T> DerefMut for StaticLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

impl<T> Drop for StaticLockReadGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ReleaseSRWLockShared(self.lock.as_ptr());
        }
    }
}

impl<T> Drop for StaticLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ReleaseSRWLockExclusive(self.lock.as_ptr());
        }
    }
}

unsafe impl<T: Send + StaticPtr> Send for StaticLock<T> {}

unsafe impl<T: Send + Sync + StaticPtr> Sync for StaticLock<T> {}

unsafe impl<T: Send> Send for StaticLockReadGuard<'_, T> {}

unsafe impl<T: Send + Sync> Sync for StaticLockReadGuard<'_, T> {}

unsafe impl<T: Send> Send for StaticLockWriteGuard<'_, T> {}

unsafe impl<T: Send + Sync> Sync for StaticLockWriteGuard<'_, T> {}
