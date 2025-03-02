use std::ffi::CString;
use std::slice::SliceIndex;
use std::ops::{Deref, DerefMut};
use std::marker::PhantomData;
use libc;

#[repr(transparent)]
pub struct LiquidVecRef<'alloc, 'data> {
    alloc: &'alloc mut BumpAlloc,
    _data: PhantomData<&'data()>,
}

impl <'alloc, 'data> LiquidVecRef<'alloc, 'data> {
    /// ```compile_fail
    /// use Freeze::{BumpAlloc};
    /// let mut allocb = BumpAlloc::new();
    /// let mut alloc = allocb.to_ref();
    /// let mut v1 = alloc.top();
    /// v1.extend_from_slice(&[42]);
    /// let slice = v1.freeze();
    /// drop(allocb);
    /// let _ = slice.len(); // should fail
    /// ```
    /// Consume the vector and produce a slice that can still be used; it's length is now fixed
    #[inline(always)]
    pub fn freeze(self) -> &'data mut [u8] {
        unsafe {
            let ret = std::ptr::slice_from_raw_parts_mut(self.alloc.top_base, self.alloc.top_size);

            self.alloc.top_base = self.alloc.top_base.add(self.alloc.top_size);
            self.alloc.top_size = 0;

            &mut *ret
        }
    }

    #[inline(always)]
    fn extend_one(&mut self, item: u8) {
        unsafe {
            *self.alloc.top_base.add(self.alloc.top_size) = item;
            self.alloc.top_size += 1;
        }
    }

    #[inline(always)]
    fn extend_reserve(&mut self, additional: usize) {
        unsafe {
            libc::madvise(self.alloc.top_base.add(self.alloc.top_size) as _, additional, libc::MADV_WILLNEED);
        }
    }

    #[inline(always)]
    pub fn extend_from_slice(&mut self, items: &[u8]) {
        unsafe {
            std::ptr::copy(items.as_ptr(), self.alloc.top_base.add(self.alloc.top_size), items.len());
            self.alloc.top_size += items.len();
        }
    }

    #[inline(always)]
    pub fn extend_from_within<R>(&mut self, src: R) where R : std::slice::SliceIndex<[u8], Output = [u8]> {
        unsafe {
            self.extend_from_slice(&std::slice::from_raw_parts(self.alloc.top_base, self.alloc.top_size).as_ref()[src])
        }
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Option<u8> {
        if self.alloc.top_size == 0 {
            None
        } else {
            unsafe {
                self.alloc.top_size -= 1;
                Some(std::ptr::read(self.alloc.top_base.add(self.alloc.top_size)))
            }
        }
    }

    #[inline(always)]
    pub fn truncate(&mut self, len: usize) {
        if len > self.alloc.top_size {
            return;
        }
        self.alloc.top_size = len
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.alloc.top_size
    }
}

impl <'alloc, 'data> std::borrow::Borrow<[u8]> for LiquidVecRef<'alloc, 'data> {
    #[inline(always)]
    fn borrow(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self.alloc.top_base, self.alloc.top_size)
        }
    }
}

impl <'alloc, 'data> std::borrow::BorrowMut<[u8]> for LiquidVecRef<'alloc, 'data> {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(self.alloc.top_base, self.alloc.top_size)
        }
    }
}

impl <'alloc, 'data> Extend<u8> for LiquidVecRef<'alloc, 'data>  {
    #[inline(always)]
    fn extend<T: IntoIterator<Item=u8>>(&mut self, iter: T) {
        iter.into_iter().for_each(|b| self.extend_one(b))
    }
}

impl <'alloc, 'data, I: SliceIndex<[u8]>> std::ops::Index<I> for LiquidVecRef<'alloc, 'data>  {
    type Output = I::Output;
    #[inline(always)]
    fn index(&self, index: I) -> &Self::Output { std::ops::Index::index(self.deref(), index) }
}

impl <'alloc, 'data, I: SliceIndex<[u8]>> std::ops::IndexMut<I> for LiquidVecRef<'alloc, 'data>  {
    #[inline(always)]
    fn index_mut(&mut self, index: I) -> &mut Self::Output { std::ops::IndexMut::index_mut(self.deref_mut(), index) }
}

impl <'alloc, 'data> std::ops::Deref for LiquidVecRef<'alloc, 'data> {
    type Target = [u8];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe {
            std::slice::from_raw_parts(self.alloc.top_base, self.alloc.top_size)
        }
    }
}

impl <'alloc, 'data> std::ops::DerefMut for LiquidVecRef<'alloc, 'data> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            std::slice::from_raw_parts_mut(self.alloc.top_base, self.alloc.top_size)
        }
    }
}


pub struct BumpAlloc {
    address_space: usize,
    data_base: *mut u8,
    top_base: *mut u8,
    top_size: usize
}

impl BumpAlloc {
    /// New Bump allocator with at most ~4GB of stuff in it
    pub fn new() -> Self {
        Self::new_with_address_space(32)
    }

    /// New Bump allocator with at most ~2^bits stuff in it
    pub fn new_with_address_space(bits: u8) -> Self {
        use libc::*;
        unsafe {
            //let res = mmap(std::ptr::null_mut(), 1 << bits, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
            let res = mmap(std::ptr::null_mut(), 1 << bits, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if res as i64 == -1 {
                let cstring = todo!();// strerror(*__errno_location());
                panic!("{:?}", CString::from_raw(cstring));
                // Err(std::str::from_utf8_unchecked(std::slice::from_raw_parts_mut(cstring, strlen(cstring)));
            }
            if res as i64 == 0 {
                panic!("mmap returned nullptr")
            }
            BumpAlloc {
                address_space: 1 << bits,
                data_base: res as *mut u8,
                top_base: res as *mut u8,
                top_size: 0,
            }
        }
    }
    pub fn to_ref<'data>(&'data mut self) -> BumpAllocRef<'data> {
        BumpAllocRef { ptr: self as *mut BumpAlloc, _data: PhantomData }
    }
}

#[repr(transparent)]
pub struct BumpAllocRef<'data> {
    ptr: *mut BumpAlloc,
    _data: PhantomData<&'data ()>,
}

impl<'data> BumpAllocRef<'data> {
    /// ```compile_fail
    /// use Freeze::{BumpAlloc};
    /// let mut alloc = BumpAlloc::new();
    /// let mut alloc = alloc.to_ref();
    /// let mut v1 = alloc.top();
    /// let mut v2 = alloc.top();
    /// v1.extend_from_slice(&[1]); // borrowing alloc twice
    /// v2.extend_from_slice(&[1]);
    /// ```
    /// Gets the (custom) Vec ref that's currently able to be modified
    pub fn top<'alloc>(&'alloc mut self) -> LiquidVecRef<'alloc, 'data> {
        unsafe {
            LiquidVecRef {
                alloc: self.ptr.as_mut().unwrap_unchecked(),
                _data: PhantomData,
            }
        }
    }

    unsafe fn data_range(&self) -> &[u8] {
        let data_base = (*self.ptr).data_base;
        std::slice::from_raw_parts(data_base, self.data_size())
    }

    unsafe fn data_range_mut(&mut self) -> &mut [u8] {
        let data_base = (*self.ptr).data_base;
        std::slice::from_raw_parts_mut(data_base, self.data_size())
    }

    /// The total number of data bytes allocated over the lifetime of the allocator
    pub fn data_size(&self) -> usize {
        unsafe {
            (*self.ptr).top_base.offset_from((*self.ptr).data_base) as usize
                + (*self.ptr).top_size
        }
    }

    /// More than half of the address space is already used
    pub fn dangerous(&self) -> bool {
        unsafe {
            (self.data_size() + size_of::<BumpAlloc>()) > (*self.ptr).address_space/2
        }
    }
}

impl<'data> Drop for BumpAllocRef<'data> {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as _, (*self.ptr).address_space);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis() {
        let mut alloc = BumpAlloc::new();
        let mut alloc = alloc.to_ref();

        let s1: &mut [u8] = {
            let mut v1 = alloc.top();
            v1.extend_from_slice(&[1, 2, 3]);
            v1.extend_one(4);
            v1.extend_from_within(..3);
            v1.deref_mut().reverse();
            v1.pop();
            v1.freeze()
        };

        assert_eq!(s1, [3, 2, 1, 4, 3, 2]);

        let s2: &mut [u8] = {
            let mut v1 = alloc.top();
            v1.extend_from_slice(&[10, 20, 30]);
            v1.extend_one(40);
            v1.extend_from_within(..3);
            v1.deref_mut().reverse();
            v1.pop();
            v1.freeze()
        };

        assert_eq!(s2, [30, 20, 10, 40, 30, 20]);
        assert_eq!(alloc.data_size(), (s1.len() + s2.len()));
    }
}
