//! Types related to task management
use super::TaskContext;
use crate::config::{MAX_SYSCALL_NUM, TRAP_CONTEXT_BASE};
use crate::mm::{
    kernel_stack_position, MapPermission, MemorySet, PhysPageNum, VirtAddr, VirtPageNum,
    KERNEL_SPACE,
};
use crate::trap::{trap_handler, TrapContext};

/// The task control block (TCB) of a task.
pub struct TaskControlBlock {
    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// The phys page number of trap context
    pub trap_cx_ppn: PhysPageNum,

    /// The size(top addr) of program which is loaded from elf file
    pub base_size: usize,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,

    /// Syscall counters indexed by syscall id
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
}

impl TaskControlBlock {
    /// get the trap context
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    /// get the user token
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// Based on the elf info in program, build the contents of task in a new address space
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;
        // map a kernel-stack in kernel space
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(app_id);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W,
        );
        let task_control_block = Self {
            task_status,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
            heap_bottom: user_sp,
            program_brk: user_sp,
            syscall_times: [0; MAX_SYSCALL_NUM],
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
    /// change the location of the program break. return None if failed.
    pub fn change_program_brk(&mut self, size: i32) -> Option<usize> {
        let old_break = self.program_brk;
        let new_brk = self.program_brk as isize + size as isize;
        if new_brk < self.heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            self.memory_set
                .shrink_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        } else {
            self.memory_set
                .append_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            self.program_brk = new_brk as usize;
            Some(old_break)
        } else {
            None
        }
    }

    /// Map [start, start+len) into this task's address space with user-specified prot.
    /// prot: bit0=readable, bit1=writable, bit2=executable (user-level semantics).
    /// Returns false (without side effects) if parameters are invalid or any page
    /// in the range is already mapped.
    pub fn mmap(&mut self, start: usize, len: usize, prot: usize) -> bool {
        // prot must only use the low 3 bits and must not be empty
        if prot & !0x7 != 0 || prot & 0x7 == 0 {
            return false;
        }
        let start_va = VirtAddr(start);
        let end_va = VirtAddr(start + len);
        // check that no page in [start, start+len) is already mapped
        let mut vpn = start_va.floor();
        let end_vpn = end_va.ceil();
        while vpn < end_vpn {
            if self.memory_set.translate(vpn).map_or(false, |pte| pte.is_valid()) {
                return false;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        // translate user prot bits to kernel MapPermission (R=1<<1, W=1<<2, X=1<<3)
        // and always add the U bit so that user mode can access the mapping
        let mut perm = MapPermission::U;
        if prot & 0x1 != 0 {
            perm |= MapPermission::R;
        }
        if prot & 0x2 != 0 {
            perm |= MapPermission::W;
        }
        if prot & 0x4 != 0 {
            perm |= MapPermission::X;
        }
        self.memory_set.insert_framed_area(start_va, end_va, perm);
        true
    }

    /// Unmap the region [start, start+len). The region must exactly match a
    /// previously mapped area (same start, same range). Returns false otherwise.
    pub fn munmap(&mut self, start: usize, len: usize) -> bool {
        // start must be page-aligned
        if start % 0x1000 != 0 {
            return false;
        }
        let start_va = VirtAddr(start);
        let end_va = VirtAddr(start + len);
        // check that every page in [start, start+len) is currently mapped
        let mut vpn = start_va.floor();
        let end_vpn = end_va.ceil();
        while vpn < end_vpn {
            if !self.memory_set.translate(vpn).map_or(false, |pte| pte.is_valid()) {
                return false;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        // remove the whole area starting at `start`
        if !self.memory_set.shrink_to(start_va, start_va) {
            return false;
        }
        // verify the requested range is now fully unmapped; if the range
        // extended beyond the removed area, some pages remain mapped
        let mut vpn = start_va.floor();
        let end_vpn = end_va.ceil();
        while vpn < end_vpn {
            if self.memory_set.translate(vpn).map_or(false, |pte| pte.is_valid()) {
                return false;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        true
    }
}

#[derive(Copy, Clone, PartialEq)]
/// task status: UnInit, Ready, Running, Exited
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}
