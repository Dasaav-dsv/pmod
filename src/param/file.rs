//! Raw param file introspection.
//! 
//! Param row manipulation uses a free list approach with
//! amortized O(1) insertion and removal performance.
//! 
//! Original implementation idea by tremwil.

use std::{
    alloc::{GlobalAlloc, Layout},
    borrow::Cow,
    error, fmt, mem,
    ptr::NonNull,
    slice,
};

use crate::stdalloc::DLStdAllocator;

const MAX_ROW_COUNT: usize =
    (i32::MAX as usize - mem::size_of::<FileHeader>()) / mem::size_of::<RowDescriptor24>();

/// The header of a param file, which contains the param table.
///
/// The param table can be manipulated in-place or may need reallocating.
#[repr(C)]
pub struct FileHeader {
    strings_offset: u32,
    _unk04: u16,
    _unk06: u16,
    version: u16,
    row_count: u16,
    table_name: FileNameUnion,
    endianness: u8,
    layout_flags: u8,
    format_flags: u8,
    _unk2f: u8,
    data_offset: u64,
    _unk38: u32,
    _unk3c: u32,
}

/// Possible param file manipulation errors.
#[derive(Clone, Copy, Debug)]
pub enum Error {
    /// The file format is improper for its encoding.
    Malformed,

    /// Row id exceeds [`i32::MAX`], is negative.
    NegativeId,

    /// Entry is not present.
    NotInTable,

    /// File needs to be reallocated with [`FileHeader::clone_reallocate`] before it can
    /// support insertion and deletion of rows.
    NeedsRealloc,

    /// Could not reallocate file.
    FailedRealloc,
}

/// Param file manipulation result.
pub type Result<T> = std::result::Result<T, Error>;

#[repr(C)]
union FileNameUnion {
    inline_name: [u8; 32],
    offset_name: FileNameOffset,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FileNameOffset {
    _unk0c: u32,
    offset: u32,
    _unk14: [u32; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RowDescriptor12 {
    pub id: u32,
    pub data_offset: u32,
    pub name_offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RowDescriptor24 {
    pub id: u32,
    pub data_offset: u64,
    pub name_offset: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LutEntry {
    pub id: u32,
    pub index: i32,
}

impl FileHeader {
    /// The name of the param table.
    ///
    /// # Errors:
    /// - [`Error::Malformed`] if the name is not valid UTF-16/SJIS.
    pub fn name<'a>(&'a self) -> Result<Cow<'a, str>> {
        let raw_name = unsafe { self.raw_name() };

        let (name, _, is_err) = if !self.is_new_layout() {
            encoding_rs::SHIFT_JIS.decode(raw_name)
        } else {
            encoding_rs::UTF_16LE.decode(raw_name)
        };

        is_err.then_some(name).ok_or(Error::Malformed)
    }

    /// The number of rows in the param table lookup table.
    ///
    /// # Errors:
    /// - [`Error::Malformed`] if the number of rows exceeds [`i32::MAX`].
    #[inline]
    pub fn row_count(&self) -> Result<usize> {
        // SAFETY: alignment of `Self` is greater than that of `i32`
        unsafe {
            usize::try_from(*(self.file_base().byte_sub(12) as *const i32))
                .map_err(|_| Error::Malformed)
        }
    }

    /// Searches for a row by its id with a binary search, returning a pointer to its data.
    ///
    /// `id` must be a non-negative signed 32-bit integer.
    ///
    /// # Errors:
    /// - [`Error::NegativeId`] if `id` is negative.
    /// - [`Error::NotInTable`] if the corresponding row is not found.
    /// - [`Error::Malformed`] if param file can't be parsed.
    pub fn find_row(&self, id: i32) -> Result<NonNull<u8>> {
        let id = u32::try_from(id).map_err(|_| Error::NegativeId)?;
        let index = self.descriptor_index_by_id(id)?;

        unsafe {
            let descriptor_base = self.file_base().byte_add(self.row_descriptor_offset()?);

            let descriptor: &dyn ReadRowDescriptor = if self.is_large_mode() {
                &*(descriptor_base as *const RowDescriptor24).add(index)
            } else {
                &*(descriptor_base as *const RowDescriptor12).add(index)
            };

            let (descriptor_id, data_offset) = descriptor.read();

            if descriptor_id == id {
                NonNull::new(self.file_base().wrapping_byte_add(data_offset))
                    .ok_or(Error::Malformed)
            } else {
                Err(Error::NotInTable)
            }
        }
    }

    /// Tries to insert a new row with fields pointed to by `data`
    /// and returns its positive id.
    ///
    /// `data` must be valid for the lifetime of the param file.
    ///
    /// # Errors:
    /// - [`Error::NeedsRealloc`] if insertion can only happen after a reallocation.
    /// - [`Error::Malformed`] if popping from the free list returned an invalid entry.
    pub fn insert_row(&mut self, data: NonNull<u8>) -> Result<i32> {
        if !self.is_large_mode() {
            return Err(Error::NeedsRealloc);
        }

        let entry = pop_free_lut_entry(self.lut_mut())?;
        let inserted_id = i32::try_from(entry.id).map_err(|_| Error::Malformed)?;

        let file_base = self.file_base();
        let data_offset = usize::wrapping_sub(data.as_ptr() as _, file_base as _) as u64;

        let index = usize::try_from(entry.index).map_err(|_| Error::Malformed)?;

        unsafe {
            let descriptor = &mut *self.large_descriptor_base().add(index);

            if descriptor.id == entry.id {
                descriptor.data_offset = data_offset;

                Ok(inserted_id)
            } else {
                Err(Error::Malformed)
            }
        }
    }

    /// Searches for a row by its id with a binary search and replaces its fields,
    /// returning a pointer to its old field data.
    ///
    /// `id` must be a non-negative signed 32-bit integer.
    ///
    /// # Errors:
    /// - [`Error::NegativeId`] if `id` is negative.
    /// - [`Error::NotInTable`] if the corresponding row is not found.
    /// - [`Error::NeedsRealloc`] if replacement can only happen after a reallocation.
    /// - [`Error::Malformed`] if param file can't be parsed.
    pub fn replace_row(&mut self, id: i32, data: NonNull<u8>) -> Result<NonNull<u8>> {
        if !self.is_large_mode() {
            return Err(Error::NeedsRealloc);
        }

        let id = u32::try_from(id).map_err(|_| Error::NegativeId)?;
        let index = self.descriptor_index_by_id(id)?;

        let file_base = self.file_base();
        let data_offset = usize::wrapping_sub(data.as_ptr() as _, file_base as _) as u64;

        unsafe {
            let descriptor = &mut *self.large_descriptor_base().add(index);

            NonNull::new(
                file_base
                    .wrapping_byte_add(mem::replace(&mut descriptor.data_offset, data_offset) as _),
            )
            .ok_or(Error::Malformed)
        }
    }

    /// Searches for a row by its id with a binary search and deletes it,
    /// returning a pointer to its old field data.
    ///
    /// `id` must be a non-negative signed 32-bit integer.
    ///
    /// # Errors:
    /// - [`Error::NegativeId`] if `id` is negative.
    /// - [`Error::NeedsRealloc`] if deletion can only happen after a reallocation.
    /// - [`Error::Malformed`] if pushing to the free list returned an invalid entry.
    pub fn delete_row(&mut self, id: i32) -> Result<NonNull<u8>> {
        if !self.is_large_mode() {
            return Err(Error::NeedsRealloc);
        }

        let id = u32::try_from(id).map_err(|_| Error::NegativeId)?;

        let mut index = self.descriptor_index_by_id(id)?;
        index = push_free_lut_entry(self.lut_mut(), index)?;

        unsafe {
            let descriptor = &mut *self.large_descriptor_base().add(index);

            NonNull::new(
                self.file_base()
                    .wrapping_byte_add(descriptor.data_offset as _),
            )
            .ok_or(Error::Malformed)
        }
    }

    /// Returns whether the file is encoded in little endian byte order.
    pub fn is_le(&self) -> bool {
        self.endianness != 0xFF
    }

    /// Returns whether the strings in the file are encoded as UTF-16 of SJIS.
    pub fn is_utf16(&self) -> bool {
        self.format_flags & 1 != 0
    }

    /// Returns whether the file supports 64-bit addressing.
    pub fn is_64bit(&self) -> bool {
        self.format_flags & 2 != 0
    }

    /// Returns whether the file uses 64-bit addressing.
    pub fn is_large_mode(&self) -> bool {
        self.is_64bit() && (self.layout_flags & 0x7f == 4 || self.layout_flags == 0x85)
    }

    /// Returns whether the file uses the "new" layout format version.
    pub fn is_new_layout(&self) -> bool {
        self.layout_flags & 0x80 != 0
    }

    fn file_base(&self) -> *mut u8 {
        self as *const _ as _
    }

    fn lut<'a>(&'a self) -> &'a [LutEntry] {
        unsafe { self.raw_lut().as_ref() }
    }

    fn lut_mut<'a>(&'a self) -> &'a mut [LutEntry] {
        unsafe { self.raw_lut().as_mut() }
    }

    fn row_descriptor_offset(&self) -> Result<usize> {
        match self.layout_flags & 0x7f {
            2 => Ok(0x30),
            3 => Ok(0x40),
            4 => Ok(0x40),
            5 => Ok(0x40),
            _ => Err(Error::Malformed),
        }
    }

    fn descriptor_index_by_id(&self, id: u32) -> Result<usize> {
        let lut = self.lut();
        let entry = find_lut_entry(lut, id).ok_or(Error::NotInTable)?;

        let index = usize::try_from(entry.index).map_err(|_| Error::Malformed)?;

        if index > lut.len() {
            return Err(Error::NotInTable);
        }

        Ok(index)
    }

    /// Clone and reallocate a file, removing duplicate rows and fixing anomalies.
    /// 
    /// # Errors:
    /// - [`Error::FailedRealloc`] if the allocator returned null.
    pub fn clone_reallocate(&self, grow: bool) -> Result<(&'static mut Self, usize)> {
        // Account for `u32::MAX` special entry
        let has_extra = self.lut().last().is_some_and(|e| e.id == u32::MAX);

        let old_len = Ord::min(
            self.row_count().unwrap_or(0) - has_extra as usize,
            MAX_ROW_COUNT,
        );

        let new_len = {
            let mut len = old_len;

            if grow {
                len = Ord::max(len * 2, 32)
            }

            Ord::clamp(old_len, len, MAX_ROW_COUNT)
        };

        let new_size = mem::size_of::<Self>() + new_len * mem::size_of::<RowDescriptor24>();
        let new_lut_size = (new_len + 1) * mem::size_of::<LutEntry>();

        let old_file_base = self.file_base();
        let old_descriptor_base = old_file_base.wrapping_byte_add(self.row_descriptor_offset()?);

        let new_file_base = unsafe {
            let new_file_base = DLStdAllocator::default().alloc_zeroed(
                Layout::from_size_align_unchecked(0x10 + new_size + new_lut_size, 16),
            );

            if new_file_base.is_null() {
                return Err(Error::FailedRealloc);
            }

            new_file_base.byte_add(0x10)
        };

        let new_row_count = Ord::min(new_len, u16::MAX as usize) as u16;

        // SAFETY: `new_file_base` is properly aligned and not null
        unsafe {
            // Layouts below 3 do not have the `data_offset` field
            if self.layout_flags <= 2 {
                let data_offset = if self.is_large_mode() {
                    old_len * mem::size_of::<RowDescriptor24>()
                } else {
                    old_len * mem::size_of::<RowDescriptor12>()
                };

                *new_file_base.cast() = Self {
                    row_count: new_row_count,
                    data_offset: usize::wrapping_sub(
                        old_file_base.wrapping_byte_add(data_offset) as _,
                        new_file_base as _,
                    ) as u64,
                    ..Default::default()
                };
            } else {
                *new_file_base.cast() = Self {
                    row_count: new_row_count,
                    data_offset: usize::wrapping_sub(
                        old_file_base.wrapping_byte_add(self.data_offset as _) as _,
                        new_file_base as _,
                    ) as u64,
                    ..Default::default()
                };
            }
        }

        let descriptor_offset = |index| unsafe {
            let descriptor: &dyn ReadRowDescriptor = if self.is_large_mode() {
                &*(old_descriptor_base as *const RowDescriptor24).add(index)
            } else {
                &*(old_descriptor_base as *const RowDescriptor12).add(index)
            };
            descriptor.read_offset()
        };

        let mut new_lut = unsafe {
            slice::from_raw_parts_mut(
                new_file_base.byte_add(new_size) as *mut LutEntry,
                new_len + 1,
            )
            .into_iter()
        };

        let mut new_descriptors = unsafe {
            slice::from_raw_parts_mut(
                new_file_base.byte_add(0x40) as *mut RowDescriptor24,
                new_len,
            )
            .into_iter()
        };

        let mut prev_id = u32::MAX;

        let mut inserted = 0;
        let mut not_inserted = new_len - old_len;

        let mut free_index = !i32::MIN;

        for entry in &self.lut()[..old_len] {
            if entry.id == prev_id || entry.index as usize >= MAX_ROW_COUNT {
                continue;
            }

            while not_inserted != 0 && prev_id.saturating_add(1) < entry.id {
                prev_id += 1;

                // SAFETY: guarded by `not_inserted`:
                // `old_len <= old_len + not_inserted <= new_descriptors.len()`
                unsafe {
                    *new_lut.next().unwrap_unchecked() = LutEntry {
                        id: prev_id,
                        index: !free_index as _,
                    };

                    new_descriptors.next().unwrap_unchecked().id = prev_id;
                }

                free_index = inserted;

                inserted += 1;
                not_inserted -= 1;
            }

            let data_offset = usize::wrapping_sub(
                old_file_base.wrapping_byte_add(descriptor_offset(entry.index as _)) as _,
                new_file_base as _,
            ) as u64;

            // SAFETY: guarded by `not_inserted`:
            // `old_len <= old_len + not_inserted <= new_descriptors.len()`
            unsafe {
                *new_lut.next().unwrap_unchecked() = LutEntry {
                    id: entry.id,
                    index: inserted as i32,
                };

                let descriptor = new_descriptors.next().unwrap_unchecked();

                descriptor.id = entry.id;
                descriptor.data_offset = data_offset;
            }

            prev_id = entry.id;

            inserted += 1;
        }

        let new_file = unsafe {
            *new_file_base.byte_sub(16).cast() = new_size as i32;
            *new_file_base.byte_sub(12).cast() = inserted - not_inserted as i32 + 1;

            &mut *(new_file_base as *mut FileHeader)
        };

        match new_file.lut_mut().last_mut() {
            Some(last) if last.id == u32::MAX => {
                last.index = free_index as _;
            }
            _ => {
                *new_lut.next().expect("insufficient length") = LutEntry {
                    id: u32::MAX,
                    index: free_index as _,
                };
            }
        }

        Ok((new_file, new_size))
    }

    unsafe fn raw_name(&self) -> &[u8] {
        let utf16_name = self.is_utf16() && self.is_new_layout();

        unsafe {
            let (ptr, max) = if !self.is_new_layout() {
                (self.table_name.inline_name.as_ptr(), 32)
            } else {
                let offset = self.table_name.offset_name.offset;
                (
                    (self as *const _ as *const u8).wrapping_byte_add(offset as usize),
                    usize::MAX,
                )
            };

            let mut len = 0;

            if !utf16_name {
                while len < max && *ptr.byte_add(len) != 0 {
                    len += 1;
                }
            } else {
                while len < max && ptr.byte_add(len).cast::<u16>().read_unaligned() != 0 {
                    len += 2;
                }
            }

            slice::from_raw_parts(ptr, len)
        }
    }

    unsafe fn raw_lut(&self) -> NonNull<[LutEntry]> {
        let file_base = self.file_base() as *const i32;

        if let Ok(offset) = usize::try_from(file_base.byte_sub(16).read_unaligned()) {
            let aligned_offset = offset.wrapping_add(15) & usize::wrapping_neg(16);
            let len = file_base.byte_sub(12).read_unaligned().max(0) as usize;

            NonNull::slice_from_raw_parts(
                unsafe { NonNull::new_unchecked(file_base.byte_add(aligned_offset) as _) },
                len,
            )
        } else {
            // SAFETY: properly aligned zero-sized slice
            NonNull::slice_from_raw_parts(
                unsafe { NonNull::new_unchecked(mem::align_of::<LutEntry>() as *mut _) },
                0,
            )
        }
    }

    /// SAFETY: [`Self::is_large_mode`] must be true
    unsafe fn large_descriptor_base(&self) -> *mut RowDescriptor24 {
        debug_assert!(self.is_large_mode(), "file must be in large mode");
        (self.file_base() as *mut RowDescriptor24).byte_add(0x40)
    }
}

fn find_lut_entry<'a>(lut: &'a [LutEntry], id: u32) -> Option<&'a LutEntry> {
    match lut.binary_search_by_key(&id, |e| e.id) {
        Ok(index) => lut.get(index),
        Err(_) => None,
    }
}

/// Pushes an entry to the free list.
///
/// Requires at least one reallocation that inserts a special entry with id `u32::MAX`
/// that keeps track of the next free list entry.
fn push_free_lut_entry<'a>(lut: &'a mut [LutEntry], index: usize) -> Result<usize> {
    let (next, rest) = lut
        .split_last_mut()
        .filter(|e| e.0.id == u32::MAX)
        .ok_or(Error::NeedsRealloc)?;

    // The index of the descriptor of the pushed entry is the binary NOT of the
    // index of the next free entry, and the value at `next.index` is the same as its index
    let pushed = rest.get_mut(index).ok_or(Error::NotInTable)?;
    let next_index = mem::replace(&mut next.index, index as i32);

    let free_index = mem::replace(&mut pushed.index, !next_index);

    usize::try_from(free_index).map_err(|_| Error::Malformed)
}

/// Tries to pop an entry from the free list.
///
/// Requires at least one reallocation that inserts a special entry with id `u32::MAX`
/// that keeps track of the next free list entry.
fn pop_free_lut_entry<'a>(lut: &'a mut [LutEntry]) -> Result<&'a mut LutEntry> {
    let (next, rest) = lut
        .split_last_mut()
        .filter(|e| e.0.id == u32::MAX)
        .ok_or(Error::NeedsRealloc)?;

    // The index of the descriptor of the popped entry is the same as its index,
    // and the value at `next.index` is the binary NOT of the index of the next free entry
    let popped = rest.get_mut(next.index as usize).ok_or(Error::NotInTable)?;
    let not_next_index = mem::replace(&mut popped.index, next.index);

    next.index = !not_next_index;

    Ok(popped)
}

trait ReadRowDescriptor {
    fn read(&self) -> (u32, usize);
    fn read_offset(&self) -> usize;
}

impl ReadRowDescriptor for RowDescriptor12 {
    #[inline]
    fn read(&self) -> (u32, usize) {
        (self.id, self.data_offset as _)
    }

    #[inline]
    fn read_offset(&self) -> usize {
        self.data_offset as _
    }
}

impl ReadRowDescriptor for RowDescriptor24 {
    #[inline]
    fn read(&self) -> (u32, usize) {
        (self.id, self.data_offset as _)
    }

    #[inline]
    fn read_offset(&self) -> usize {
        self.data_offset as _
    }
}

impl Default for FileHeader {
    fn default() -> Self {
        Self {
            strings_offset: 0,
            _unk04: 0,
            _unk06: 0,
            version: 1,
            row_count: 0,
            table_name: FileNameUnion {
                offset_name: Default::default(),
            },
            endianness: 0,
            layout_flags: 0x85,
            format_flags: 3,
            _unk2f: 0,
            data_offset: 0,
            _unk38: 0,
            _unk3c: 0,
        }
    }
}

impl Default for FileNameOffset {
    fn default() -> Self {
        Self {
            _unk0c: 0,
            offset: 0,
            _unk14: [0; 6],
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self, f)
    }
}

impl error::Error for Error {}
