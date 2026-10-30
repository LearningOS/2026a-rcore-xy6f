# Lab1 实验报告：多道程序与分时多任务（ch3）

## 荣誉准则（Honor Code）

请把填写了《你的说明》下述内容拷贝到实验报告中。否则，你的提交将视作无效，本次实验的成绩将按"0"分计。

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   *《无》*

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   *《rCore 实验指导（https://learningos.github.io/rCore-Tutorial-Guide/）第三章；rCore-Tutorial-Book-v3（https://rcore-os.github.io/rCore-Tutorial-Book-v3/）第三章；课程仓库（2026a-rcore-xy6f）提供的实验框架代码》*

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。
   我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。
   我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。
   我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。
   我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按"-100"分计。

---

## 一、实验总结（200 字以内）

本实验在 ch3 多道程序与分时多任务系统的基础上，实现了系统调用 `sys_trace`（ID 410）的三种功能：根据 `trace_request` 的值，读取用户内存地址处的一个字节（0）、向用户内存地址写入一个字节（1）、查询当前任务某系统调用的累计调用次数（2），且查询动作本身也计入统计；非法请求返回 -1。为此，在任务控制块（TCB）中新增了系统调用计数表和任务首次运行时间字段，并在系统调用分发入口统一记账。通过全部测例。

---

## 二、指导书题目与解答

### 2.1 编程作业：获取任务信息（实现 `sys_trace`）

#### 题目（摘自实验指导第三章）

在 ch3 中，我们的系统已经能够支持多个任务分时轮流运行，我们希望引入一个新的系统调用 `sys_trace`（ID 为 410）用来追踪当前任务系统调用的历史信息，并做对应的修改。定义如下：

```rust
fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize
```

**调用规范：**

- 这个系统调用有三种功能，根据 `trace_request` 的值不同，执行不同的操作：
- 如果 `trace_request` 为 0，则 `id` 应被视作 `*const u8`，表示读取当前任务 `id` 地址处一个字节的无符号整数值。此时应忽略 `data` 参数。返回值为 `id` 地址处的值。
- 如果 `trace_request` 为 1，则 `id` 应被视作 `*mut u8`，表示写入 `data`（作为 `u8`，即只考虑最低位的一个字节）到该用户程序 `id` 地址处。返回值应为 0。
- 如果 `trace_request` 为 2，表示查询当前任务调用编号为 `id` 的系统调用的次数，返回值为这个调用次数。**本次调用也计入统计**。
- 否则，忽略其他参数，返回值为 -1。

**说明：**

- 这个调用的读写并不安全，使用不当可能导致崩溃。这是因为在下一章节实现地址空间之前，系统中缺乏隔离机制。所以我们 **不要求你实现安全检查机制，只需通过测试用例即可**。
- 这个系统调用读写本任务内存的功能并不是很有用。这是因为作业的灵感来源 syscall 主要依靠 trace 功能追踪其他任务的信息，但在本章节我们还没有进程、线程等概念，所以简化了操作，只要求追踪自身的信息。

**提示：**

- 大胆修改已有框架！除了配置文件，你几乎可以随意修改已有框架的内容。
- 系统调用次数可以考虑在内核态的 `syscall` 函数中统计。
- 可以扩展 `TaskManagerInner` 中的结构来维护新的信息。
- 不要害怕使用 `unsafe` 做类型转换，这在内核处理用户调用时是不可避免的。
- 在实现时，可以把系统调用参数中前缀的下划线去掉，这样更清晰。

**实验要求：**

- 完成分支：ch3。
- 实验目录要求：`os`（内核实现）与 `reports` 目录结构。
- 通过所有测例：CI 使用的测例与本地相同。默认情况下，makefile 仅编译基础测例（`BASE=1`），可在 os 目录执行 `make run BASE=0` 仅编译实验测例，或指定 `BASE=2` 同时编译基础测例和实验测例。

#### 实现说明

**（1）在 TCB 中增加计数表与启动时刻字段**

计数查询要求统计"从任务开始运行至今"的调用次数，这是一段跨越任务整个生命周期的状态，因此将其保存在与任务同生共死的 TCB 中：

```rust
// os/src/task/task.rs
pub struct TaskControlBlock {
    pub task_status: TaskStatus,
    pub task_cx: TaskContext,
    pub syscall_times: [u32; MAX_SYSCALL_NUM], // 系统调用计数表：下标 = 调用号
    pub start_time: usize,                     // 任务首次运行的时刻（tick）
}
```

`MAX_SYSCALL_NUM = 500` 定义在 `os/src/config.rs`，大于用户库当前最大的系统调用号（473），留有余量。选用定长数组是因为系统调用号是有界小整数，数组下标寻址为 O(1)，且内核处于 `#![no_std]` 环境，没有现成的 HashMap 可用。

**（2）在系统调用分发入口统一记账**

记账点选择在分发函数 `syscall()` 的 `match` 之前，而非各个 `sys_xxx` 函数内部，原因有三：

1. `syscall()` 是所有系统调用的唯一必经点，一处记账即可覆盖全部调用，未来新增系统调用无需修改；
2. 非法系统调用号（走 `_ => panic!` 分支）没有对应处理函数，但按语义同样应被统计；
3. 记账先于分派执行，因此 `sys_trace` 自身的查询动作也会被计入——测试中 `assert_eq!(2, count_syscall(SYSCALL_TRACE))` 正是依赖此语义。

```rust
// os/src/syscall/mod.rs
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    TASK_MANAGER.increase_current_syscall_times(syscall_id);
    match syscall_id {
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_GET_TIME => sys_get_time(args[0] as *mut TimeVal, args[1]),
        SYSCALL_TRACE => sys_trace(args[0], args[1], args[2]),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
```

访问 TCB 通过 `TaskManager` 提供的方法，借用模式与既有方法一致——用 `exclusive_access()` 获取内部可变借用，并将借用窗口压缩在方法体内：

```rust
// os/src/task/mod.rs
pub fn increase_current_syscall_times(&self, syscall_id: usize) {
    let mut inner = self.inner.exclusive_access();
    let current = inner.current_task;
    if syscall_id < inner.tasks[current].syscall_times.len() {
        inner.tasks[current].syscall_times[syscall_id] += 1;
    }
}

pub fn get_current_syscall_times(&self, syscall_id: usize) -> u32 {
    let inner = self.inner.exclusive_access();
    let current = inner.current_task;
    inner.tasks[current].syscall_times[syscall_id]
}
```

其中 `if syscall_id < len` 是防御性检查：系统调用号来自用户输入，任何用户输入都不应导致内核数组越界崩溃。

**（3）记录任务启动时刻**

`start_time` 记录任务首次被调度为 Running 的时刻，在 `run_first_task()`（任务 0 首次运行）与 `run_next_task()`（其余任务首次运行）两处写入，并以 `start_time == 0` 作为"尚未运行过"的哨兵值保证只记录一次：

```rust
inner.tasks[next].task_status = TaskStatus::Running;
if inner.tasks[next].start_time == 0 {
    inner.tasks[next].start_time = get_time();
}
inner.current_task = next;
```

**（4）`sys_trace` 的具体实现**

```rust
// os/src/syscall/process.rs
const TRACE_READ: usize = 0;
const TRACE_WRITE: usize = 1;
const TRACE_SYSCALL: usize = 2;

pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        TRACE_SYSCALL => TASK_MANAGER.get_current_syscall_times(id) as isize,
        TRACE_READ => {
            if id == 0 {
                return -1;
            }
            unsafe { (id as *const u8).read_volatile() as isize }
        }
        TRACE_WRITE => {
            if id == 0 {
                return -1;
            }
            unsafe { (id as *mut u8).write_volatile(data as u8) };
            0
        }
        _ => -1,
    }
}
```

**关于直通用户内存**：Read/Write 分支直接将用户传来的地址 `id` 转换为指针并解引用。这在 ch3 是可行的——本章尚未引入虚拟内存（ch4 才引入页表），内核与所有用户程序运行在同一套物理地址空间中，用户程序中 `&var` 的地址值在内核视角下指向同一块物理内存，因此可以直通访问。

使用 `read_volatile` / `write_volatile` 而非普通读写，是因为 release 模式下编译器可能将"结果无人消费的写入"或"可被合并的读取"优化删除，而 trace 语义要求访存真实发生（测试在写入后跨系统调用边界用 `read_volatile` 验证副作用）。volatile 访存向编译器声明：此操作有可观测副作用，禁止删除与重排。

---

### 2.2 简答作业

#### 第 1 题

**题目：** 正确进入 U 态后，程序的特征还应有：使用 S 态特权指令，访问 S 态寄存器后会报错。请同学们可以自行测试这些内容（运行三个 bad 测例 `ch2b_bad_*.rs`），描述程序出错行为，同时注意注明你使用的 sbi 及其版本。

**解答：**

本实验使用的 SBI 为随仓库提供的 **RustSBI** 固件（`bootloader/rustsbi-qemu.bin`，版本 **0.4.1**），由 QEMU 以 `-bios` 方式加载。三个 bad 测例的行为如下：

- **`ch2b_bad_address`**：用户程序向地址 `0x0` 执行写操作（`(0x0 as *mut u8).write_volatile(0)`）。地址 0 不在该应用可访问的内存范围内，CPU 触发访存错误（StoreFault / StorePageFault）。陷入内核后，`trap_handler` 的 `StoreFault | StorePageFault` 分支打印：

  ```
  [kernel] PageFault in application, bad addr = ..., bad instruction = ..., kernel killed it.
  ```

  随后调用 `exit_current_and_run_next()` 终止该任务并调度下一个任务。

- **`ch2b_bad_instructions`**：用户程序在 U 模式下执行 S 模式特权指令 `sret`。U 模式不允许执行特权指令，CPU 触发非法指令异常（IllegalInstruction），内核打印：

  ```
  [kernel] IllegalInstruction in application, kernel killed it.
  ```

  并终止该任务。

- **`ch2b_bad_register`**：用户程序在 U 模式下执行 `csrr x, sstatus` 读取 S 态寄存器 `sstatus`。U 模式访问 S 态 CSR 同样属于非法操作，触发非法指令异常，内核打印 `[kernel] IllegalInstruction in application, kernel killed it.` 并终止该任务。

三个测例共同验证了 U 态的特权隔离：用户程序一旦使用 S 态特权指令或访问 S 态资源，硬件会立即触发异常，内核捕获后终止该应用并继续运行其他任务，整个系统不会崩溃。

#### 第 2 题

**题目：** 深入理解 `trap.S` 中两个函数 `__alltraps` 和 `__restore` 的作用，并回答如下问题：

**2.1 L40：刚进入 `__restore` 时，`sp` 代表了什么值。请指出 `__restore` 的两种使用情景。**

**解答：**

刚进入 `__restore` 时，`sp` 指向**内核栈上的一个 TrapContext**——即 `__alltraps` 分配并保存的 34×8 字节的完整现场（对于正常 trap），或者是首次进入用户态前由 `init_app_cx` 预置在内核栈顶的"伪造现场"。

`__restore` 的两种使用情景：

1. **正常 trap 返回**：用户程序发生 `ecall`、时钟中断或异常后，`__alltraps` 保存现场并调用 `trap_handler` 处理完毕，代码自然落入 `__restore`，恢复现场后 `sret` 回到用户态。
2. **首次进入用户态**：新任务第一次被调度时，其 `TaskContext` 由 `TaskContext::goto_restore(init_app_cx(i))` 构造——`ra` 被设为 `__restore` 的地址，`sp` 被设为假 TrapContext 的地址。`__switch` 加载该上下文后执行 `ret`，直接跳入 `__restore`。`__restore` 把这个"伪造的现场"当作真实现场处理，从而完成内核态到用户态的第一次切换。

**2.2 L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的值对于进入用户态有何意义？请分别解释。**

**解答：**

L43-L48 特殊处理了 `sstatus`、`sepc` 和用户栈指针（x2）三个值：

- `ld t0, 32*8(sp); csrw sstatus, t0`：恢复 `sstatus`。其中 `SPP` 位决定了 `sret` 之后 CPU 进入的特权级（此处为 User）；`SPIE` 位用于 `sret` 时恢复 `SIE`，保证返回用户态后时钟中断仍然能够触发。
- `ld t1, 33*8(sp); csrw sepc, t1`：恢复 `sepc`。`sret` 会跳转到 `sepc` 指向的地址执行，因此 `sepc` 决定了返回用户态后从哪条指令继续执行——对系统调用而言是 `ecall` 的下一条指令，对首次进入用户态的新任务而言是应用入口地址。
- `ld t2, 2*8(sp); csrw sscratch, t2`：从 TrapContext 的 x[2] 取出用户栈指针，暂存到 `sscratch`。这是为最后的 `csrrw sp, sscratch, sp` 交换做准备：用户栈指针不能直接写回 `sp`，否则会丢失内核栈的位置。

**2.3 L50-L56：为何跳过了 `x2` 和 `x4`？**

**解答：**

- **x2（sp）**：用户栈指针不在这里直接恢复，而是由 L45 读到 `sscratch`、再由 L60 的 `csrrw sp, sscratch, sp` 交换回 `sp`，同时把内核栈顶保存在 `sscratch` 中。若此处直接 `ld x2, 2*8(sp)`，则 `sp` 变成用户栈指针，后续既无法继续访问内核栈上的 TrapContext，也无法完成内核栈/用户栈的交换。
- **x4（tp）**：`tp` 是线程指针（Thread Pointer），rCore 的用户程序不使用 `tp` 寄存器，因此无需恢复。

**2.4 L60：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？（对应代码 `csrrw sp, sscratch, sp`）**

**解答：**

`csrrw sp, sscratch, sp` 是原子交换指令，执行后：

- `sp` = 交换前 `sscratch` 的值 = **用户栈指针**；
- `sscratch` = 交换前 `sp` 的值 = **内核栈顶**。

这样 `sret` 进入用户态后，`sp` 指向用户栈供应用使用；而 `sscratch` 中预存了内核栈顶。当用户程序下一次发生 trap 时，`__alltraps` 的第一条指令 `csrrw sp, sscratch, sp` 又能把 `sp` 换回内核栈，从而在内核栈上保存现场。

**2.5 `__restore` 中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？**

**解答：**

状态切换发生在最后一条指令 `sret`（L61）。`sret` 是 S 模式特权返回指令，硬件会执行：

```
pc          = sepc
sstatus.SIE  = sstatus.SPIE
sstatus.SPIE = 1
sstatus.SPP  = 0
```

由于此前 `sepc` 已恢复为用户程序地址、`sstatus.SPP` 已设置为 User，`sret` 执行后 CPU 的 `pc` 跳到用户程序地址、特权级降到 U 模式，因此进入用户态。

**2.6 L13：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？（对应代码 `csrrw sp, sscratch, sp`）**

**解答：**

L13 是 `__alltraps` 的第一条指令 `csrrw sp, sscratch, sp`。执行后：

- `sp` = 交换前 `sscratch` 的值 = **内核栈顶**；
- `sscratch` = 交换前 `sp` 的值 = **用户栈指针**。

这样内核在进入 trap 的一瞬间就获得了可用的内核栈来保存现场，同时把用户栈指针暂存在 `sscratch` 中，稍后由 L34-L35 存入 TrapContext 的 x[2] 字段。

**2.7 从 U 态进入 S 态是哪一条指令发生的？**

**解答：**

对于系统调用/异常，是**用户程序执行的 `ecall` 指令**：用户程序执行 `ecall` 后，硬件将 `sepc` 设为该指令地址、`scause` 写入异常原因、`sstatus.SPP` 置为 U，并将 `pc` 设为 `stvec`（即 `__alltraps` 的地址），从而 CPU 从 U 模式陷入 S 模式。

对于时钟中断等异步中断，则是硬件中断机制触发：CLINT 检测到 `time >= mtimecmp` 后产生定时器中断，OpenSBI 将其转发为 S 态时钟中断，CPU 同样跳转到 `stvec`。因此，严格地说，U→S 的切换发生在 `ecall` 指令（软件触发）或硬件中断响应（异步触发）的瞬间，硬件统一将 `pc` 设置为 `stvec` 指向的 `__alltraps`。

---

## 三、实验验证

1. 本地构建并运行：在仓库根目录执行 `make -C os run BASE=1`，`ch3_trace`、`ch3_sleep`、`ch3_sleep1` 及 `ch3b_yield*` 全部通过，输出符合预期。
2. 官方检查器：执行 `make -C ci-user test CHAPTER=3`，`check/ch3.py` 检查的 `get_time OK`、`Test sleep OK`、`current time_msec`、`Test sleep1 passed`、`string from task trace test`、`Test trace OK` 全部出现，测试通过。
3. 提交后 GitHub Actions 评测通过，ch3 成绩 7/7（100 分）。

## 四、实验心得（可选）

本次实验的核心工作不是写 `sys_trace` 的三个 match 分支本身，而是为"查询"搭建前置机制：确定状态的归属载体（TCB）、选择记账的收口位置（系统调用分发入口）、在借用规则内安全访问任务数据（UPSafeCell 模式）。这三个决策体现了操作系统设计中"状态归属""机制收口""防御边界"的通用原则，也将在后续章节的地址空间（ch4）与进程管理（ch5）中以更复杂的形态重现。