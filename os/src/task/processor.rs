//!Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::{fetch_task, TaskStatus};
use super::{TaskContext, TaskControlBlock};
use crate::mm::{MapPermission, VirtAddr};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;
use crate::timer::get_time_ms;
use crate::config::{BIG_STRIDE, MAX_SYSCALL_NUM};
use crate::syscall::{TaskInfo, SYSCALL_TONG};

/// Processor management structure
pub struct Processor {
    ///The task currently executing on the current processor
    current: Option<Arc<TaskControlBlock>>,

    ///The basic control flow of each core, helping to select and switch process
    idle_task_cx: TaskContext,
}

impl Processor {
    ///Create an empty Processor
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }

    ///Get mutable reference to `idle_task_cx`
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }

    ///Get current task in moving semanteme
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }

    ///Get current task in cloning semanteme
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }
}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

///The main part of process execution and scheduling
///Loop `fetch_task` to get the process that needs to run, and switch the process through `__switch`
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            // access coming task TCB exclusively
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;
            task_inner.task_status = TaskStatus::Running;

            // 当进程第一次运行的时候，更新它的开始时间
            if task_inner.start_time == -1 {
                task_inner.start_time = get_time_ms() as isize;
            }

            // 增加当前运行进程的步长
            task_inner.stride += BIG_STRIDE / task_inner.prior;

            // release coming task_inner manually
            drop(task_inner);
            // release coming task TCB manually
            processor.current = Some(task);
            // release processor manually
            drop(processor);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            warn!("no tasks available in run_tasks");
        }
    }
}

/// Get current task through take, leaving a None in its place
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

pub fn restore_current_task(curr_task: Arc<TaskControlBlock>) {
    PROCESSOR.exclusive_access().current = Some(curr_task);
}

/// Get a copy of the current task
pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// Get the current user token(addr of page table)
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.get_user_token()
}

///Get the mutable reference to trap context of current task
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

///Return to idle control flow for new scheduling
pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_cx_ptr, idle_task_cx_ptr);
    }
}

/// 增加当前运行进程所调用的系统调用的次数
pub fn add_current_syscall_cnt(syscall_id: usize) {
    if let Some((id, _)) = SYSCALL_TONG
        .iter()
        .enumerate()
        .find(|(_, &val)| syscall_id == val) 
    {
        let curr_task = take_current_task().unwrap();
        let mut inner = curr_task.inner_exclusive_access();
        inner.syscall_times[id] += 1;
        restore_current_task(curr_task.clone());
    } else {
        panic!("Unsupported syscall_id: {}", syscall_id);
    }
}

/// 获取当前运行进程的系统调用次数，以及运行的总时长
pub fn get_current_info(ti: *mut TaskInfo) {
    let curr_task = current_task().unwrap();
    let inner = curr_task.inner_exclusive_access();
    let status = inner.task_status;
    let mut syscall_times = [0; MAX_SYSCALL_NUM];
    inner.syscall_times
        .iter()
        .enumerate()
        .for_each(|(id, cnt)| {
        let syscall_id = SYSCALL_TONG[id];
        syscall_times[syscall_id] = *cnt;
    });
    let time = get_time_ms() - inner.start_time as usize;
    unsafe{
        *ti = TaskInfo {
            status,
            syscall_times,
            time
        };
    }
}

/// 修改当前运行进程的优先级
pub fn curr_set_priority(prio: isize) {
    let curr_task = take_current_task().unwrap();
    let mut inner = curr_task.inner_exclusive_access();
    inner.prior = prio as usize;
    restore_current_task(curr_task.clone());
}

/// 给当前进程新增加一块内存映射 [start_va, end_va)
pub fn curr_mmap(start_va: VirtAddr, end_va: VirtAddr, permission: MapPermission) {
    let curr_task = take_current_task().unwrap();
    let mut inner = curr_task.inner_exclusive_access();
    inner.memory_set.insert_framed_area(start_va, end_va, permission);
    restore_current_task(curr_task.clone());
}