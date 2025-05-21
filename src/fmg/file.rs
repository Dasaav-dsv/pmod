use std::{
    alloc::{GlobalAlloc, Layout},
    mem,
    num::{NonZeroU32, NonZeroU64},
    ptr::{self, NonNull},
    slice,
};

use crate::stdalloc::DLStdAllocator;

pub const MAX_MSG_COUNT: u32 =
    (u32::MAX - mem::size_of::<FileHeader>() as u32) / mem::size_of::<MsgGroup>() as u32;

#[repr(C)]
pub struct FileHeader {
    _unk00: u8,
    endianness: u8,
    version: u16,
    file_size: u32,
    _unk08: u32,
    group_count: u32,
    msg_count: u32,
    max_group_size: u32,
    msg_offsets: NonNull<Option<NonZeroU64>>,
    _unk20: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MsgGroup {
    offset: u32,
    first_id: u32,
    last_id: u32,
    _unk0c: u32,
}

impl FileHeader {
    pub fn msg_index_by_id(&self, id: u32) -> Option<u32> {
        let groups = self.msg_groups();

        let mut left = 0;
        let mut right = self.group_count as usize - 1;

        if id < groups[left].first_id || id > groups[right].last_id {
            return None;
        }

        while left <= right {
            let mid = (left + right) / 2;
            let group = &groups[mid];

            if group.last_id < id {
                left = mid + 1;
            } else {
                if group.first_id <= id {
                    return Some(id - group.first_id + group.offset);
                }

                right = mid - 1;
            }
        }

        None
    }

    pub fn msg_data_by_index(&self, index: u32) -> Option<NonNull<u16>> {
        let offsets =
            unsafe { slice::from_raw_parts(self.msg_offsets.as_ptr(), self.msg_count as _) };

        let offset = *offsets.get(index as usize)?;

        NonNull::new(self.file_base().wrapping_byte_add(offset?.get() as _) as _)
    }
    pub fn replace_msg_by_index(
        &mut self,
        index: u32,
        data: Option<NonNull<u16>>,
    ) -> Option<NonNull<u16>> {
        let offsets =
            unsafe { slice::from_raw_parts_mut(self.msg_offsets.as_ptr(), self.msg_count as _) };

        let offset = offsets.get_mut(index as usize)?;

        let old_data = offset
            .and_then(|o| NonNull::new(self.file_base().wrapping_byte_add(o.get() as _) as _));

        *offset = data.and_then(|data| {
            NonZeroU64::new(usize::wrapping_sub(data.as_ptr() as _, self.file_base() as _) as u64)
        });

        old_data
    }

    fn file_base(&self) -> *mut u8 {
        self as *const _ as _
    }

    fn msg_groups(&self) -> &[MsgGroup] {
        unsafe {
            slice::from_raw_parts(
                self.file_base().byte_add(mem::size_of::<Self>()) as _,
                self.group_count as _,
            )
        }
    }

    pub fn try_insert_new_after(
        &mut self,
        after: NonZeroU32,
        data: Option<NonNull<u16>>,
    ) -> Option<NonZeroU32> {
        let new_offset = data.and_then(|data| {
            NonZeroU64::new(usize::wrapping_sub(data.as_ptr() as _, self.file_base() as _) as u64)
        });

        let groups = self.msg_groups();

        let index = match groups.binary_search_by_key(&after.get(), |g| g.first_id) {
            Err(i) => i,
            Ok(i) => i,
        };

        let offsets =
            unsafe { slice::from_raw_parts_mut(self.msg_offsets.as_ptr(), self.msg_count as _) };

        for (group, range) in groups[index..].iter().filter_map(|g| {
            g.offset
                .checked_add(g.last_id.checked_sub(g.first_id)?)
                .zip(Some(g))
                .map(|(b, g)| (g, (g.offset as usize..=b as usize)))
        }) {
            for (i, offset) in offsets.get_mut(range).into_iter().flatten().enumerate() {
                if offset.is_none() {
                    *offset = new_offset;

                    return NonZeroU32::new(group.first_id + i as u32);
                }
            }
        }

        None
    }

    pub fn grow_reallocate(&self, after: NonZeroU32) -> Option<&'static mut Self> {
        let old_msg_count = self.msg_count;
        let new_msg_count = Ord::min(old_msg_count.saturating_mul(2), MAX_MSG_COUNT);

        if new_msg_count == old_msg_count {
            return None;
        }

        let max_group_size = self.max_group_size.max(1);

        let old_group_count = self.group_count;
        let new_group_count =
            old_group_count + (new_msg_count - old_msg_count).div_ceil(max_group_size);

        unsafe {
            let old_file_base = self.file_base();

            let alloc = DLStdAllocator::default();

            let new_file_size = u32::checked_add(
                mem::size_of::<Self>() as _,
                new_group_count * mem::size_of::<MsgGroup>() as u32,
            )?;

            let new_file_layout = Layout::from_size_align_unchecked(new_file_size as _, 16);
            let new_file_base = alloc.alloc(new_file_layout) as *mut Self;

            if new_file_base.is_null() {
                return None;
            }

            let new_offsets_size = new_msg_count as usize * mem::size_of::<usize>();

            let new_msg_offsets = alloc
                .alloc_zeroed(Layout::from_size_align_unchecked(new_offsets_size, 8))
                as *mut Option<NonZeroU64>;

            let Some(new_msg_offsets) = NonNull::new(new_msg_offsets) else {
                alloc.dealloc(new_file_base as _, new_file_layout);

                return None;
            };

            new_file_base.write(Self {
                file_size: new_file_size,
                msg_offsets: new_msg_offsets,
                max_group_size,
                ..Default::default()
            });

            let old_offsets = slice::from_raw_parts(self.msg_offsets.as_ptr(), old_msg_count as _);
            let new_offsets_to_init =
                slice::from_raw_parts_mut(new_msg_offsets.as_ptr(), old_msg_count as _);

            for (new, old) in new_offsets_to_init.iter_mut().zip(old_offsets) {
                if let Some(old) = old {
                    *new = NonZeroU64::new(usize::wrapping_sub(
                        old_file_base.wrapping_byte_add(old.get() as _) as _,
                        new_file_base as _,
                    ) as u64)
                }
            }

            let old_groups = self.msg_groups();
            let new_groups = new_file_base.byte_add(mem::size_of::<Self>()) as *mut MsgGroup;

            let index = match old_groups.binary_search_by_key(&after.get(), |g| g.first_id) {
                Err(i) => i,
                Ok(i) => i,
            };

            ptr::copy_nonoverlapping(old_groups.as_ptr(), new_groups, index);

            let mut not_inserted_groups = new_group_count - index as u32;
            let mut not_inserted_msgs = new_msg_count - self.msg_count;

            let mut next_group = new_groups.add(index);
            let mut next_offset = self.msg_count;

            let mut prev_last_id = old_groups.get(index).map_or(after.get(), |g| g.last_id);

            for group in &old_groups[index..] {
                while not_inserted_groups != 0
                    && not_inserted_msgs != 0
                    && prev_last_id < group.first_id - 1
                {
                    let msgs_to_insert = not_inserted_msgs
                        .min(group.first_id - 1 - prev_last_id)
                        .min(max_group_size);

                    next_group.write(MsgGroup {
                        offset: next_offset,
                        first_id: prev_last_id,
                        last_id: prev_last_id + msgs_to_insert - 1,
                        _unk0c: 0,
                    });

                    not_inserted_groups -= 1;
                    not_inserted_msgs -= msgs_to_insert;

                    prev_last_id += msgs_to_insert;
                    next_offset += msgs_to_insert;

                    next_group = next_group.add(1);
                }

                next_group.write(*group);

                not_inserted_groups -= 1;

                next_group = next_group.add(1);

                prev_last_id = group.last_id;
            }

            while not_inserted_groups != 0 && not_inserted_msgs != 0 {
                let msgs_to_insert = not_inserted_msgs.min(max_group_size);

                next_group.write(MsgGroup {
                    offset: next_offset,
                    first_id: prev_last_id + 1,
                    last_id: prev_last_id + msgs_to_insert,
                    _unk0c: 0,
                });

                not_inserted_groups -= 1;
                not_inserted_msgs -= msgs_to_insert;

                prev_last_id += msgs_to_insert;
                next_offset += msgs_to_insert;

                next_group = next_group.add(1);
            }

            (*new_file_base).group_count = new_group_count - not_inserted_groups;
            (*new_file_base).msg_count = new_msg_count - not_inserted_msgs;

            Some(&mut *new_file_base)
        }
    }
}

impl Default for FileHeader {
    fn default() -> Self {
        Self {
            _unk00: 0,
            endianness: 0,
            version: 2,
            file_size: 0,
            _unk08: 1,
            group_count: 0,
            msg_count: 0,
            max_group_size: 0xFF,
            msg_offsets: NonNull::dangling(),
            _unk20: 0,
        }
    }
}
