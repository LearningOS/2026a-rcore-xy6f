//! Process management syscalls
use crate::mm::{translated_byte_buffer, PageTable, PTEFlags, VirtAddr};
use crate::task::{
    change_program_brk, exit_current_and_run_next, get_current_syscall_times, mmap_current,
    munmap_current, suspend_current_and_run_next, current_user_token,
};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond, writing the result through the
/// user page table (TimeVal may be splitted by two pages)
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let buffers = translated_byte_buffer(
        current_user_token(),
        ts as *const u8,
        core::mem::size_of::<TimeVal>(),
    );
    if buffers.is_empty() {
        return -1;
    }
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &time_val as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    let mut offset = 0;
    for buffer in buffers {
        for byte in buffer.iter_mut() {
            *byte = bytes[offset];
            offset += 1;
        }
    }
    0
}

const TRACE_READ: usize = 0;
const TRACE_WRITE: usize = 1;
const TRACE_SYSCALL: usize = 2;

/// trace syscall: query syscall counters or read/write user memory
/// through the user page table with permission checks
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        TRACE_SYSCALL => get_current_syscall_times(id) as isize,
        TRACE_READ => {
            let page_table = PageTable::from_token(current_user_token());
            let va = VirtAddr::from(id);
            if let Some(pte) = page_table.translate(va.floor()) {
                // the page must be valid, readable, and visible to user mode (U bit)
                if pte.is_valid()
                    && pte.readable()
                    && pte.flags().contains(PTEFlags::U)
                {
                    let pa = pte.ppn().0 * 0x1000 + va.page_offset();
                    return unsafe { (pa as *const u8).read_volatile() as isize };
                }
            }
            -1
        }
        TRACE_WRITE => {
            let page_table = PageTable::from_token(current_user_token());
            let va = VirtAddr::from(id);
            if let Some(pte) = page_table.translate(va.floor()) {
                // the page must be valid, writable, and visible to user mode (U bit)
                if pte.is_valid()
                    && pte.writable()
                    && pte.flags().contains(PTEFlags::U)
                {
                    let pa = pte.ppn().0 * 0x1000 + va.page_offset();
                    unsafe { (pa as *mut u8).write_volatile(data as u8) };
                    return 0;
                }
            }
            -1
        }
        _ => -1,
    }
}

// implement mmap: map [start, start+len) with user prot into current address space
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if start % 0x1000 != 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    if mmap_current(start, len, prot) {
        0
    } else {
        -1
    }
}

// implement munmap: unmap [start, start+len) from current address space
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if len == 0 {
        return 0;
    }
    if munmap_current(start, len) {
        0
    } else {
        -1
    }
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
