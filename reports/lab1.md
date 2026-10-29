# Lab1 实验报告：多道程序与分时多任务（ch3）

## 一、实验目标

在 ch3 多道程序与分时多任务系统的基础上，实现 `sys_trace` 系统调用，使其支持三种功能：

1. 查询当前任务某系统调用的累计调用次数（`TraceRequest::Syscall`）；
2. 读取用户内存某地址处的 1 字节（`TraceRequest::Read`）；
3. 向用户内存某地址写入 1 字节（`TraceRequest::Write`）。

完成 `user/src/bin/ch3_trace.rs`、`ch3_sleep.rs` 等全部测试，并撰写本报告。

## 二、系统调用机制回顾

用户程序通过 `ecall` 指令触发 U→S 特权级切换，`trap_handler`（`os/src/trap/mod.rs`）识别 `UserEnvCall` 异常后，从 TrapContext 中取出系统调用号（寄存器 a7/x17）与参数（a0~a2/x11~x13），转交系统调用分发入口：

```rust
// os/src/syscall/mod.rs
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    TASK_MANAGER.increase_current_syscall_times(syscall_id); // 本次新增：入口记账
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

用户侧的 `TraceRequest` 枚举（`user/src/lib.rs`）定义了 `sys_trace` 第一个参数的三种取值：

```rust
#[repr(C)]
pub enum TraceRequest {
    Read,     // 0
    Write,    // 1
    Syscall,  // 2
}
```

## 三、实现设计

### 3.1 在 TCB 中增加系统调用计数表

计数查询要求统计"从任务开始运行至今"的调用次数，这是一段跨越任务整个生命周期的状态。局部变量无法跨函数调用存活，全局变量在多任务环境下无法区分归属，而 TCB（任务控制块）与任务同生共死，是唯一生命周期匹配的载体。因此在 `os/src/task/task.rs` 中为 TCB 增加两个字段：

```rust
pub struct TaskControlBlock {
    pub task_status: TaskStatus,
    pub task_cx: TaskContext,
    pub syscall_times: [u32; MAX_SYSCALL_NUM], // 系统调用计数表：下标 = 调用号
    pub start_time: usize,                     // 任务首次运行的时刻（tick）
}
```

选用定长数组的原因：系统调用号是有界小整数（用户库最大 473），数组下标寻址 O(1)，且内核运行于 `#![no_std]` 环境，无标准 HashMap 可用。数组长度 `MAX_SYSCALL_NUM = 500` 定义于 `os/src/config.rs`，留有余量。

任务创建时账本必须清零，否则读到未初始化内存会导致断言随机失败：

```rust
// os/src/task/mod.rs  lazy_static! 初始化处
let mut tasks = [TaskControlBlock {
    task_cx: TaskContext::zero_init(),
    task_status: TaskStatus::UnInit,
    syscall_times: [0; MAX_SYSCALL_NUM],
    start_time: 0,
}; MAX_APP_NUM];
```

### 3.2 在系统调用分发入口统一记账

记账点选择在分发函数 `syscall()` 的 match 之前而非各 `sys_xxx` 函数内部，理由：

1. `syscall()` 是所有系统调用的唯一必经点，一处记账覆盖全部调用，未来新增系统调用无需修改；
2. 非法系统调用号（走 `_ => panic!` 分支）没有对应的处理函数，但按语义同样应被统计；
3. 记账先于分派执行，因此 `sys_trace` 自身发起的查询也会被计入——测试中 `assert_eq!(2, count_syscall(SYSCALL_TRACE))` 正是依赖此语义。

访问 TCB 需要经过 `TaskManager` 提供的方法，借用模式与既有 `current_task()` 一致——通过 `exclusive_access()` 获取内部可变借用，借用窗口压缩在方法体内数行：

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

### 3.3 记录任务启动时刻

`start_time` 记录任务首次被调度为 Running 的时刻。在 `run_first_task()`（0 号任务首次运行）与 `run_next_task()`（其余任务首次运行）两处写入，并以 `start_time == 0` 作为"尚未运行过"的哨兵值保证幂等：

```rust
inner.tasks[next].task_status = TaskStatus::Running;
if inner.tasks[next].start_time == 0 {
    inner.tasks[next].start_time = get_time();
}
inner.current_task = next;
```

### 3.4 sys_trace 的实现

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

### 3.5 内核访问用户内存的原理

Read/Write 分支直接将用户传来的地址 `id` 转换为指针并解引用。这在 ch3 是可行的：本章系统尚未引入虚拟内存（ch4 才引入页表），内核与所有用户程序运行在同一套物理地址空间中，用户程序中 `&var` 的地址值在内核视角下指向同一块物理内存，因此可以直通访问。

使用 `read_volatile` / `write_volatile` 而非普通读写，是因为 release 模式下编译器可能将"结果无人消费的写入"或"可被合并的读取"优化删除，而 trace 语义要求访存真实发生（测试在写入后跨系统调用边界用 `read_volatile` 验证副作用）。volatile 访存向编译器声明：此操作有可观测副作用，禁止删除与重排。

## 四、问答作业

### 4.1 `__switch` 执行前后寄存器与栈的变化

`__switch`（`os/src/task/switch.S`）是任务切换的核心，接收两个参数（a0 = current_task_cx_ptr，a1 = next_task_cx_ptr）：

```asm
__switch:
    sd ra, 0(a0)
    sd sp, 8(a0)
    sd s0, 16(a0)
    ...                    # 依次保存 s1~s11
    sd s11, 120(a0)

    ld ra, 0(a1)
    ld sp, 8(a1)
    ld s0, 16(a1)
    ...                    # 依次恢复 s1~s11
    ld s11, 120(a1)
    ret
```

执行前：当前任务运行在它自己的内核栈上，`a0` 指向当前任务 TCB 中的 TaskContext，`a1` 指向目标任务的 TaskContext，`ra` 保存着"切回来后应继续执行的地址"（`run_next_task` 中 `__switch` 调用点之后的指令）。

执行时前半段：将 `ra`、`sp`、`s0~s11` 共 14 个寄存器依次存入当前任务的 TaskContext。只保存被调用者保存寄存器（callee-saved），因为 `__switch` 是普通函数调用，调用者保存寄存器（t0~t6、a0~a7）由 Rust 编译器遵守调用约定已自动保存/无需保留。

执行时后半段：从目标任务的 TaskContext 恢复同一批寄存器——`sp` 被换成了目标任务的内核栈指针，此后代码运行在目标任务栈上。

`ret` 执行时：跳到目标任务 TaskContext 中保存的 `ra`。对从未运行过的新任务，其 `ra` 被初始化为 `__restore` 的地址（`TaskContext::goto_restore`），于是"返回"后直接走 trap 返回流程进入该任务的用户态。

### 4.2 特权级切换时 sp 与 sscratch 的作用

在 U→S 切换（`__alltraps`）时，`sscratch` 中预先存放着内核栈顶地址（指向该任务的 TrapContext）。`__alltraps` 执行 `csrrw sp, sscratch, sp`：原子地交换两者——`sp` 变为内核栈顶，`sscratch` 暂存用户栈指针；随后将用户寄存器上下文压入内核栈，形成 TrapContext。

S→U 返回（`__restore`）时反向操作：从内核栈恢复 TrapContext 的过程中，最后将 TrapContext 中保存的用户 `sp` 恢复，并通过再次 `csrrw` 将内核栈顶换回 `sscratch`，然后 `sret` 回到用户态。

设计意图：进入内核的瞬间需要立刻有一个"确定可用"的栈来保存现场，`sscratch` 就是这个预先放置的内核栈指针；两个栈（用户栈/内核栈）分离保证了用户程序无论怎么破坏自己的栈都无法影响内核现场。

### 4.3 TrapContext 保存了什么，为何 sys_trace 返回值要 as isize

TrapContext（`os/src/trap/context.rs`）保存了用户态的全部通用寄存器 x0~x31（其中 x2/sp 由 sscratch 暂存单独处理）、sstatus、sepc（触发 ecall 的指令地址，返回前 +4 跳过 ecall 本身）。

系统调用返回值经 `cx.x[10] = syscall(...) as usize` 写回 a0 寄存器。RISC-V 系统调用约定中 a0 为负数表示错误、非负表示结果，而计数结果是 u32，直接写回会被解释为极大正数或溢出，因此实现中用 `as isize` 保持有符号语义后再由 trap 层转 usize 写回。

### 4.4 从本实验看"状态归属"的设计原则

本实验的计数表、启动时刻都是"描述单个任务、跨越任务生命周期"的状态。设计时的判断标准是：数据描述谁，就放在谁的结构体里（TCB）；描述全体任务管理状态（当前任务号、任务列表）才放入 TaskManagerInner。这一原则在 ch5 进程管理（pid、父子关系挂在 PCB）中会再次应用。

## 五、实验记录

1. **定位留白**：在 `os/src/syscall/process.rs` 中找到 `// TODO: implement the syscall` 的 `sys_trace` 空壳。
2. **TCB 加字段后的连锁编译错误**：为 `TaskControlBlock` 增加 `syscall_times` 与 `start_time` 后，`task/mod.rs` 中数组构造处编译报错"missing field"，按编译器提示在初始化处补上 `[0; MAX_SYSCALL_NUM]` 与 `0`。教训：Rust 结构体新增字段时，所有构造点必须同步补齐。
3. **忘记清零的隐患**：若账本未初始化为 0，读到未定义内存值，`assert!(3 <= count_syscall(...))` 将随机失败。分配即初始化是内核代码的底线。
4. **记账位置的语义验证**：最初考虑在各 sys_xxx 内部记账，分析测试断言 `assert_eq!(2, count_syscall(SYSCALL_TRACE))` 后确认查询动作自身必须被计入，故统一放在 `syscall()` 入口 match 之前。
5. **验证**：`make -C os run BASE=1` 后 `ch3_trace`、`ch3_sleep`、`ch3_sleep1` 及 `ch3b_yield*` 全部通过，`make -C ci-user test CHAPTER=3` 全绿。

## 六、总结

本实验在 ch3 多道程序系统上完整实现 `sys_trace` 的三个子功能。核心工作不是写三个 match 分支本身，而是为"查询"搭建前置机制：确定状态的归属载体（TCB）、选择记账的收口位置（syscall 分发入口）、在借用规则内安全访问任务数据（UPSafeCell 模式）。这三个决策体现了操作系统设计中"状态归属""机制收口""防御边界"的通用原则，也将在后续章节的页表翻译（ch4）与进程管理（ch5）中以更复杂的形态重现。
