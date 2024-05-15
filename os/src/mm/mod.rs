//! Memory management implementation
//!
//! SV39 page-based virtual-memory architecture for RV64 systems, and
//! everything about memory management, like frame allocator, page table,
//! map area and memory set, is implemented here.
//!
//! Every task or process has a memory_set to control its virtual memory.

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

use address::VPNRange;
pub use address::{PhysAddr, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
pub use memory_set::{kernel_token, MapPermission, MemorySet, KERNEL_SPACE, remap_test};
pub use page_table::{PTEFlags, PageTable};
pub use page_table::{
    translated_byte_buffer, translated_ref, translated_refmut, translated_str,
    PageTableEntry, UserBuffer, UserBufferIterator,
    va_var2pa_mut, vpnrange_exist_mapped,
    vpnrange_exist_unmapped,
};

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
