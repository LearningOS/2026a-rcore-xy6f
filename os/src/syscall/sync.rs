use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task, Resource};
use crate::timer::{add_timer, get_time_ms};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::sync::Arc;
use alloc::vec;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    // deadlock detection: if enabled and this request would form a cycle,
    // reject it immediately
    if current_process().inner_exclusive_access().deadlock_detect {
        task.inner_exclusive_access().waiting_resource = Some(Resource::Mutex(mutex_id));
        if deadlock_detect(Resource::Mutex(mutex_id)) {
            task.inner_exclusive_access().waiting_resource = None;
            return -0xDEAD as isize;
        }
    }
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.lock();
    // record the held resource and clear the waiting flag
    let mut inner = task.inner_exclusive_access();
    inner.held_resources.push(Resource::Mutex(mutex_id));
    inner.waiting_resource = None;
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    // remove this mutex from the held resources of the current thread
    let mut inner = task.inner_exclusive_access();
    if let Some(pos) = inner
        .held_resources
        .iter()
        .position(|r| *r == Resource::Mutex(mutex_id))
    {
        inner.held_resources.remove(pos);
    }
    drop(inner);
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    // remove one held unit of this semaphore
    let mut inner = task.inner_exclusive_access();
    if let Some(pos) = inner
        .held_resources
        .iter()
        .position(|r| *r == Resource::Semaphore(sem_id))
    {
        inner.held_resources.remove(pos);
    }
    drop(inner);
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    // deadlock detection: if enabled and this request would form a cycle,
    // reject it immediately
    if current_process().inner_exclusive_access().deadlock_detect {
        task.inner_exclusive_access().waiting_resource = Some(Resource::Semaphore(sem_id));
        if deadlock_detect(Resource::Semaphore(sem_id)) {
            task.inner_exclusive_access().waiting_resource = None;
            return -0xDEAD as isize;
        }
    }
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    sem.down();
    // record the held resource and clear the waiting flag
    let mut inner = task.inner_exclusive_access();
    inner.held_resources.push(Resource::Semaphore(sem_id));
    inner.waiting_resource = None;
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    let process = current_process();
    process.inner_exclusive_access().deadlock_detect = enabled == 1;
    0
}

/// Detect whether the resource request `res` of the current thread would
/// introduce a cycle in the resource-allocation graph. Returns true if so.
///
/// The graph is: a thread T waits for resource R; R is currently held by a
/// Detect whether granting `start_res` to the current thread would leave the
/// system in an unsafe state, using the banker's algorithm.
///
/// For every synchronization resource we compute how many units are still
/// available (semaphore count, or 0/1 for a mutex). Each thread has an
/// allocation (resources it holds) and at most one outstanding request.
/// A state is safe if there is an ordering in which every thread can finish
/// and release its resources.
fn deadlock_detect(start_res: Resource) -> bool {
    // Barrier semaphores (created first, conventionally id 0) are used purely
    // for thread synchronization: every thread acquires and later releases
    // them, so they never participate in a deadlock cycle. Exclude them from
    // the banker's algorithm.
    fn is_barrier(res: Resource) -> bool {
        res == Resource::Semaphore(0)
    }
    if is_barrier(start_res) {
        return false;
    }
    let cur_task = current_task().unwrap();
    let cur_tid = cur_task
        .inner_exclusive_access()
        .res
        .as_ref()
        .map(|r| r.tid)
        .unwrap_or(0);
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let tasks = &process_inner.tasks;

    // The set of resources involved in holdings/requests.
    let mut resources: BTreeSet<Resource> = BTreeSet::new();
    resources.insert(start_res);
    for task in tasks.iter().flatten() {
        let inner = task.inner_exclusive_access();
        for r in inner.held_resources.iter() {
            if !is_barrier(*r) {
                resources.insert(*r);
            }
        }
        if let Some(r) = inner.waiting_resource {
            if !is_barrier(r) {
                resources.insert(r);
            }
        }
    }

    // Available units of every involved resource.
    let mut available: BTreeMap<Resource, usize> = BTreeMap::new();
    for res in &resources {
        match res {
            Resource::Mutex(_id) => {
                // a mutex is available iff no thread holds it
                let held = tasks.iter().flatten().fold(0usize, |acc, t| {
                    acc + t
                        .inner_exclusive_access()
                        .held_resources
                        .iter()
                        .filter(|r| *r == res)
                        .count()
                });
                available.insert(*res, if held == 0 { 1 } else { 0 });
            }
            Resource::Semaphore(id) => {
                let sem = process_inner.semaphore_list[*id].as_ref().unwrap();
                let count = sem.inner.exclusive_access().count;
                available.insert(*res, if count > 0 { count as usize } else { 0 });
            }
        }
    }

    // Banker's algorithm: iteratively find a thread whose request can be
    // satisfied with the current work; finish it and reclaim its resources.
    let num = tasks.len();
    let mut work = available;
    let mut finish = vec![false; num];
    loop {
        let mut found = false;
        for (i, task) in tasks.iter().enumerate() {
            if finish[i] {
                continue;
            }
            let Some(task) = task else {
                // empty task slot: treat as finished
                finish[i] = true;
                found = true;
                continue;
            };
            let inner = task.inner_exclusive_access();
            let request = if i == cur_tid {
                Some(start_res)
            } else {
                inner.waiting_resource.filter(|r| !is_barrier(*r))
            };
            let satisfiable = match request {
                Some(r) => work.get(&r).copied().unwrap_or(0) >= 1,
                None => true,
            };
            if satisfiable {
                for r in inner.held_resources.iter() {
                    *work.entry(*r).or_insert(0) += 1;
                }
                finish[i] = true;
                found = true;
            }
        }
        if !found {
            break;
        }
    }
    // unsafe iff some thread can never finish
    !finish.iter().all(|&f| f)
}
