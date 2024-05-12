//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::loader::{get_app_data, get_num_app};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use crate::timer::get_time_ms;
use crate::config::MAX_SYSCALL_NUM;
use crate::syscall::{SYSCALL_TONG, TaskInfo};
use crate::mm::{MapPermission, VPNRange, VirtAddr, VirtPageNum};
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let task0 = &mut inner.tasks[0];
        task0.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &task0.task_cx as *const TaskContext;

        if task0.start_time == -1 {
            task0.start_time = get_time_ms() as isize;
        } else {
            panic!("task0 is running");
        }
        
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;

            if inner.tasks[next].start_time == -1 {
                inner.tasks[next].start_time = get_time_ms() as isize;
            }
            
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    /// add syscall cnt of current task
    fn add_current_syscall_cnt(&self, syscall_id: usize) {
        if let Some((id, _)) = SYSCALL_TONG
            .iter()
            .enumerate()
            .find(|(_, &val)| syscall_id == val) 
        {
            let mut inner = self.inner.exclusive_access();
            let curr_id = inner.current_task;
            let curr_task = &mut inner.tasks[curr_id];
            curr_task.syscall_times[id] += 1;
        } else {
            panic!("Unsupported syscall_id: {}", syscall_id);
        }
    }

    /// get information of current task
    fn get_current_info(&self, ti: *mut TaskInfo) {
        let inner = self.inner.exclusive_access();
        let curr_task = &inner.tasks[inner.current_task];
        let status = curr_task.task_status;
        let mut syscall_times = [0; MAX_SYSCALL_NUM];
        curr_task.syscall_times.iter().enumerate().for_each(|(id, cnt)| {
            let syscall_id = SYSCALL_TONG[id];
            syscall_times[syscall_id] = *cnt;
        });
        let time = get_time_ms() - curr_task.start_time as usize;
        unsafe{
            *ti = TaskInfo {
                status,
                syscall_times,
                time
            };
        }
    }

    /// check whether a vpn has been mapped in vpnrange
    pub fn curr_vpnrange_exist_map(&self, start:VirtPageNum, end: VirtPageNum) -> bool {
        let inner = self.inner.exclusive_access();
        let curr_task = &inner.tasks[inner.current_task];
        
        let vpnrange = VPNRange::new(start, end);
        for vpn in vpnrange {
            if curr_task.memory_set.vpn_ismap(vpn) {
                return true;
            }
        }
        return false;
    }

    /// check whether a vpn has been unmapped in vpnrange
    pub fn curr_vpnrange_exist_unmap(&self, start:VirtPageNum, end: VirtPageNum) -> bool {
        let vpnrange = VPNRange::new(start, end);
        let inner = self.inner.exclusive_access();
        let curr_task = &inner.tasks[inner.current_task];
        
        for vpn in vpnrange {
            if !curr_task.memory_set.vpn_ismap(vpn) {
                return true;
            }
        }
        return false;
    }

    /// new a new area that is [start_va, end_va]
    pub fn curr_mmap(
        &self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        let mut inner = self.inner.exclusive_access();
        let curr_id = inner.current_task;
        let curr_task = &mut inner.tasks[curr_id];
        curr_task.memory_set.insert_framed_area(start_va, end_va, permission);
    }

    /// unmap [start_va, end_va]
    pub fn curr_munmap_with_start_vpn(&self, start_vpn: VirtPageNum) -> isize {
        let mut inner = self.inner.exclusive_access();
        let curr_id = inner.current_task;
        let curr_task = &mut inner.tasks[curr_id];
        curr_task.memory_set.remove_area_with_start_vpn(start_vpn)
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// Add syscall times of current 'Running' task
pub fn add_current_syscall_cnt(syscall_id: usize) {
    TASK_MANAGER.add_current_syscall_cnt(syscall_id);
}

/// Get info of current task
pub fn get_current_info(ti: *mut TaskInfo) {
    TASK_MANAGER.get_current_info(ti);
}

/// check whether a vpn has been mapped in vpnrange of current task
pub fn curr_vpnrange_exist_map(start: VirtPageNum, end: VirtPageNum) -> bool {
    TASK_MANAGER.curr_vpnrange_exist_map(start, end)
}

/// check whether a vpn has been unmapped in vpnrange of current task
pub fn curr_vpnrange_exist_unmap(start: VirtPageNum, end: VirtPageNum) -> bool {
    TASK_MANAGER.curr_vpnrange_exist_unmap(start, end)
}

/// alloc a new area that is [start_va, end_va]
pub fn curr_mmap(start: usize, len: usize, mut port: usize) -> isize {
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    // println!("start: {:x}, end: {:x}", start_va.0, end_va.0);
    if !start_va.aligned()
    || (port & !0x7) != 0 || (port & 0x7) == 0 
    || curr_vpnrange_exist_map(start_va.floor(), end_va.ceil()) {
        -1
    } else {
        port <<= 1;
        let mut permission = MapPermission::from_bits(port as u8).unwrap();
        permission = permission | MapPermission::U;
        TASK_MANAGER.curr_mmap(start_va, end_va, permission);
        0
    }
}

/// unmap a area that is starting in start_va
pub fn curr_munmap(start: usize, len: usize) -> isize {
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    if !start_va.aligned() || curr_vpnrange_exist_unmap(start_va.floor(), end_va.ceil()) {
        -1
    } else {
        TASK_MANAGER.curr_munmap_with_start_vpn(start_va.floor())
    }
}