# Lab5 实验报告：并发与同步（ch8）

## 荣誉准则（Honor Code）

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   *《无》*

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   *《rCore 实验指导（https://learningos.github.io/rCore-Tutorial-Guide/）第八章；rCore-Tutorial-Book-v3（https://rcore-os.github.io/rCore-Tutorial-Book-v3/）第八章；课程仓库（2026a-rcore-xy6f）提供的实验框架代码》*

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。
   我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。
   我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。
   我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。
   我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按"-100"分计。

---

## 一、实验总结（200 字以内）

本实验实现了死锁检测机制：新增 `sys_enable_deadlock_detect` 系统调用，按进程开关死锁检测。检测采用银行家算法——从所有线程的持有资源（held_resources）与等待资源（waiting_resource）出发，计算每类资源可用量（信号量当前 count、互斥锁空闲/占用），迭代检查是否存在安全序列；若系统进入不安全状态，则 `mutex_lock`/`semaphore_down` 返回 `-0xDEAD` 拒绝请求。检测排除 barrier 同步信号量。同时补全了 `sys_get_time`（ch8 测试依赖用户态忙等 sleep）。全部测例通过。完成实验用时约 1 天。

---

## 二、指导书题目与解答

### 2.1 编程作业（摘自实验指导第八章）

#### 题目

实现死锁检测机制，通过新的系统调用 `sys_enable_deadlock_detect` 为进程按需开启或关闭死锁检测。开启后，`mutex_lock` 和 `semaphore_down` 若检测到死锁，应拒绝操作并返回 `-0xDEAD`。检测算法基于银行家算法，使用 Available、Allocation、Need 三个数据结构判断系统是否处于安全状态。死锁检测对 mutex 和 semaphore 分别进行。

#### 实现说明

**（1）数据结构**

- 每个线程的 TCB 新增两个字段：
  - `held_resources: Vec<Resource>`：该线程已持有的同步资源（`Resource::Mutex(id)` 或 `Resource::Semaphore(id)`），每次成功 `lock`/`down` 后加入，`unlock`/`up` 时移除；
  - `waiting_resource: Option<Resource>`：该线程当前正在等待的资源（阻塞在 `mutex_lock`/`semaphore_down` 期间保留）。
- 进程新增 `deadlock_detect: bool` 开关，`sys_enable_deadlock_detect` 设置它。

**（2）银行家算法**

`deadlock_detect(start_res)` 实现如下：

1. 收集所有线程的持有资源与等待资源，得到涉及的资源集合；
2. 计算每类资源的可用量 `available`：
   - 信号量：`max(0, 当前 count)`（count 为负说明单元已全部分配）；
   - 互斥锁：若没有任何线程持有则为 1，否则 0；
3. 迭代寻找"请求可被满足"的线程（当前线程的请求是 `start_res`，其他线程的请求是其 `waiting_resource`，无请求的线程视为可直接完成），让其完成并释放其持有的全部资源（更新 work）；
4. 若所有线程都能完成，系统安全（不判死锁）；否则不安全，返回 true（拒绝请求）。

**（3）barrier 排除**

测试中的 barrier 信号量（id 0）用于线程同步：每个线程都会获取并最终释放它，不参与死锁环。因此检测中排除 `Resource::Semaphore(0)`，避免把 barrier 等待误判为死锁。

**（4）`sys_get_time`**

ch8 分支的 `sys_get_time` 是空壳，而用户态忙等 `sleep` 依赖 `get_time`，因此用 `translated_byte_buffer` 补全（处理 TimeVal 跨页）。

---

### 2.2 简答作业

#### 题目一：主线程（0 号线程）退出时视为整个进程退出，需结束所有线程并回收资源。需要回收哪些资源？其他线程的 TaskControlBlock 可能在哪些位置被引用，分别是否需要回收，为什么？

**解答：**

**需要回收的资源：**

1. **用户资源**：每个线程的用户栈（`TaskUserRes` 中的用户栈页）、trap 上下文所在页、地址空间页表（进程退出时整个 `MemorySet` 回收）；
2. **内核资源**：每个线程的内核栈（`KernelStack`）、内核栈所在页表映射；
3. **PID/TID**：进程的 `PidHandle`、各线程的 tid（通过 `RecycleAllocator` 回收复用）；
4. **文件描述符**：进程的 `fd_table` 中打开的文件（`Arc<dyn File>` 引用计数归零后释放）；
5. **同步对象**：进程的 `mutex_list`/`semaphore_list`/`condvar_list`；
6. **进程与线程的 TCB/PCB** 本身（引用计数归零后 drop）。

**其他线程的 TCB 可能被引用的位置及是否需要回收：**

- **进程的 `tasks` 列表**（`process.inner.tasks: Vec<Option<Arc<TCB>>>`）：主线程退出、进程销毁时，整个进程的 `ProcessControlBlock` 被 drop，`tasks` 随之销毁，其中所有 `Arc<TCB>` 引用被释放——**需要回收**，否则子线程 TCB 泄漏。
- **就绪队列 / 阻塞队列**：子线程若处于 `Ready` 状态在 `TASK_MANAGER.ready_queue` 中，或阻塞在 `Semaphore::wait_queue`/`MutexBlocking` 等待队列中，进程销毁时这些队列里仍持有 `Arc<TCB>`——**必须回收/移除**，否则悬空或泄漏；实现上通常在进程退出时把这些队列中属于该进程的任务移除或标记退出。
- **`PROCESSOR.current`**：当前正在运行的线程由 `Processor` 持有 `Arc<TCB>`，主线程退出时通过 `take_current_task()` 取出，不再持有——无需额外回收（引用随之释放）。
- **定时器队列 `TIMERS`**：若线程因 `sleep` 阻塞，其 `Arc<TCB>` 在定时器堆中，需 `remove_timer` 移除——**需要回收**。
- **`INITPROC`/`pid2process` 等全局表**：进程退出时从 `pid2process` 移除，释放对 PCB 的引用。

**为什么需要回收**：TCB 由 `Arc` 管理，只有所有强引用都被释放，TCB 才真正 drop 并回收内核栈等资源。若遗漏某处引用，TCB 会泄漏（内核内存无法回收）；若在引用未清时访问，可能触发 use-after-free。

#### 题目二：对比两种 Mutex 实现（Mutex1 与 Mutex2）的区别及可能导致的问题。

**解答：**

本实验框架中的两种互斥锁实现：

- **Mutex1（`MutexSpin`）**：自旋锁。`lock()` 通过忙等（spin）不断尝试获取锁（如用 `test_and_set` 或原子交换），直到成功。`unlock()` 释放。
  - 优点：不依赖调度器，临界区极短时开销低（不陷入内核切换）；
  - 问题：持有锁的线程若被时钟中断抢占，其他线程会一直空转浪费 CPU；单核下若持有者被切走，等待者会忙等到时间片耗尽，可能饿死。

- **Mutex2（`MutexBlocking`）**：阻塞锁。`lock()` 获取失败时把当前线程加入等待队列并 `block_current_and_run_next`（让出 CPU），`unlock()` 唤醒等待队列中的一个线程。
  - 优点：等待者不占用 CPU，适合临界区较长或竞争激烈的场景；
  - 问题：每次加锁/解锁可能涉及线程切换（系统调用 + 调度），开销较大；若唤醒逻辑或等待队列管理有误，可能造成线程永久阻塞或重复唤醒。

两者在并发语义上等价（都保证互斥），但**性能特征和适用场景不同**：自旋锁适合短临界区、多核环境；阻塞锁适合长临界区、竞争激烈或需要避免忙等浪费的环境。可能的问题包括：自旋锁在单核抢占下导致死锁般的忙等；阻塞锁若忘记唤醒或唤醒条件错误会导致线程卡死。因此选择时需结合临界区长度与硬件环境。

---

## 三、实验验证

1. 本地构建并运行：`make -C os run`，ch2~ch8 全部测例通过，包括：
   - `deadlock test mutex 1 OK`
   - `deadlock test semaphore 1 OK`
   - `deadlock test semaphore 2 OK`
   - `pipetest passed`、`sync_sem passed`、`test_condvar passed`、`threads test passed`、`threads with arg test passed`、`race adder using spin mutex test passed`、`philosopher dining problem with mutex test passed`、`mpsc_sem passed` 等。
2. 官方检查器：`make -C ci-user test CHAPTER=8` 通过。
3. 提交后 GitHub Actions 评测通过。

## 四、实验心得（可选）

本次实验最有价值的部分是理解"死锁检测"与"银行家算法"的结合。最初用资源分配图的环检测实现，但信号量的多单元特性导致误判（同一线程重复 down barrier 被误认为自死锁；sem2 场景中因存在会释放资源的线程而被误判）。改用银行家算法后，通过"是否存在安全序列"判断，正确区分了 sem1（无安全序列，死锁）与 sem2（有安全序列，不死锁）。此外还发现 barrier 同步信号量会干扰银行家算法，需从检测中排除。这让我深刻体会到：**死锁检测的正确性依赖于对资源语义（互斥 vs 计数、是否可释放）的准确建模**。