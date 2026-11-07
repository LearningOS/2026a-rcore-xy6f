# Lab3 实验报告：进程管理（ch5）

## 荣誉准则（Honor Code）

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   *《无》*

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   *《rCore 实验指导（https://learningos.github.io/rCore-Tutorial-Guide/）第五章；rCore-Tutorial-Book-v3（https://rcore-os.github.io/rCore-Tutorial-Book-v3/）第五章；课程仓库（2026a-rcore-xy6f）提供的实验框架代码》*

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。
   我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。
   我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。
   我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。
   我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按"-100"分计。

---

## 一、实验总结（200 字以内）

本实验在 ch4 地址空间基础上引入进程模型。移植了 `sys_get_time`、`sys_mmap`、`sys_munmap` 到 ch5 的 TCB 结构（通过 `current_task()` 访问当前进程的 `MemorySet` 完成页表翻译与权限检查）。实现了 `sys_spawn`：将 fork 的"创建进程"与 exec 的"加载新程序"合并，直接以新程序创建子进程。实现了 `sys_set_priority` 与 stride 调度：TCB 记录 priority/pass/stride，调度器改为选取 stride 最小者，进程让出时按 `pass = BigStride/priority` 累加。全部测例通过。

---

## 二、指导书题目与解答

### 2.1 编程作业（摘自实验指导第五章）

#### 题目一：实现 `sys_spawn`

在已有 `fork`、`exec` 等系统调用基础上，实现 `spawn` 系统调用（`sys_spawn`，ID 400）：

```rust
fn sys_spawn(path: *const u8) -> isize
```

它结合了 fork 与 exec 的功能：直接给定一个程序路径，创建一个运行该程序的新子进程。

> 注意：spawn 与 fork 在地址空间隔离机制上有本质区别。

#### 题目二：实现 stride 调度

在 ch3 基础上实现带优先级的 stride 调度：

1. 为每个任务维护一个 stride 值，表示已运行时间累计量，每运行一个 tick，stride 增加 `pass`。
2. 每次需要调度时，从所有 runnable 任务中选择 stride 最小的任务。
3. 一个时间片后，回到第 2 步。

若 `P.pass = BigStride / P.priority`，则其中 `BigStride` 取一个比所有 stride 最大值大 2 的整数，可保证不会溢出。

新增系统调用 `sys_set_priority(prio: isize) -> isize`（ID 140），用于设置当前任务的优先级（要求 `prio >= 2`，失败返回 -1）。

#### 实现说明

**（1）移植 `sys_get_time` / `sys_mmap` / `sys_munmap`**

ch5 的进程结构改为 `Arc<TaskControlBlock>`（外层）+ `TaskControlBlockInner`（内层），没有 ch4 的 `mmap_current`/`munmap_current` 辅助函数。因此直接在系统调用里通过 `current_task()` 拿当前进程，再 `inner_exclusive_access()` 访问其 `memory_set`：

- `sys_get_time`：用 `translated_byte_buffer(current_user_token(), ts, 16)` 翻译用户虚拟地址（处理 TimeVal 跨页），逐字节写入。
- `sys_mmap`：校验 `start` 页对齐、`prot` 合法（低 3 位且非全 0）、区间无重叠，然后把用户 `prot`（bit0=R, bit1=W, bit2=X）换算成内核 `MapPermission`（R=1<<1, W=1<<2, X=1<<3）并加 `U` 位，调用 `insert_framed_area`。
- `sys_munmap`：校验 `start` 页对齐、区间每页已映射，`shrink_to(start, start)` 整区删除后复查全清，否则返回 -1。

**（2）实现 `sys_spawn`**

在 TCB 上新增 `spawn(self: &Arc<Self>, elf_data: &[u8]) -> Arc<Self>` 方法：用 `MemorySet::from_elf` 从新程序构建全新地址空间（不是 fork 那样复制父进程地址空间），分配 pid 和内核栈，构造子进程 TCB，并把子进程加入父进程的 `children`。`sys_spawn` 里用 `translated_str` 读用户传来的路径，`get_app_data_by_name` 找到程序数据，创建子进程后 `add_task` 加入调度队列，返回子进程 pid。

> 与 fork 的本质区别：fork 复制父进程的地址空间，spawn 直接加载新程序，地址空间完全不同。

**（3）实现 stride 调度 + `sys_set_priority`**

- TCB inner 新增三个字段：`stride: usize`（已运行累计量）、`pass: usize`（每次时间片增量）、`priority: usize`（优先级）。新进程默认 `priority = 16`，`pass = BIG_STRIDE / 16`，其中 `BIG_STRIDE = 1 << 32`。
- `sys_set_priority(prio)`：`prio >= 2` 时设置 `priority` 并重算 `pass = BIG_STRIDE / priority`，返回 `prio`；否则返回 -1（测例要求 `set_priority(isize::MAX)` 也成功返回本身）。
- 调度器：`TaskManager::fetch()` 从 FIFO 改为遍历就绪队列，选取 `stride` 最小的进程。
- 让出时累加：`suspend_current_and_run_next()` 中 `stride = stride.wrapping_add(pass)`。

这样，高优先级进程 `pass` 小，`stride` 增长慢，因而被更频繁地选中；低优先级进程 `pass` 大，增长快，被选中的频率低。各进程获得的 CPU 时间与 `priority` 成正比。实测 6 个进程（priority 5~10）的 `count/priority` 比值基本相等，公平性满足要求。

---

### 2.2 简答作业

#### 第 1 题：使用 `fork + exec` 这一组合时，能否直接实现一个新的系统调用？若不能，请说明并给出一个简单可行方案。

**解答：**

可以，但要付出代价。若直接用 `fork + exec` 组合来实现"创建并运行新程序"，需要用户程序**两次**陷入内核（先 fork 再 exec），且 fork 会复制父进程的地址空间——如果父进程地址空间很大（大量代码/数据/堆），复制开销巨大，而 exec 立刻又用新程序覆盖它，白白浪费一次复制。此外用户还要自行维护子进程生命周期。

更简单的方案是引入一个**新的系统调用 `spawn(path)`**：一次系统调用完成"创建新进程 + 加载新程序"。内核直接根据给定路径从可执行文件构建新的地址空间（`MemorySet::from_elf`），不复制父进程地址空间，创建好子进程后加入调度队列。这就是本实验实现的 `sys_spawn`（ID 400）。它既避免了无意义的地址空间复制，又把两步操作合并成一步，用户侧更简洁。

#### 第 2 题：描述 stride 调度的实现细节，给出一个实际调度场景的 pass 计算示例。

**解答：**

stride 调度的核心是"谁跑得少，就让谁跑"。每个进程维护三个量：

- `priority`：用户设置的优先级（>= 2）；
- `pass`：每次被调度运行一个时间片后，`stride` 增加的量，`pass = BigStride / priority`；
- `stride`：累计运行量，每运行一个时间片 `stride += pass`。

调度器每次从就绪队列中选取 `stride` 最小的进程运行。

**示例**（使用 8 位无符号 stride，`BigStride = 255`）：

设两个进程 `P1.priority = 1`、`P2.priority = 2`：

```
pass1 = BigStride / priority1 = 255 / 1 = 255
pass2 = BigStride / priority2 = 255 / 2 = 127（整数除法，向下取整）
初始：p1.stride = 255，p2.stride = 250
```

初始 `p2.stride = 250 < p1.stride = 255`，因此调度器选择 **P2** 运行一个时间片。时间片结束后：

```
p2.stride = p2.stride + pass2 = 250 + 127 = 377
```

此时 `p1.stride = 255`，`p2.stride = 377`，调度器下一次选择 `p1`（stride 更小）。可见 P2 的优先级是 P1 的 2 倍，其 stride 增长速率只有 P1 的一半，因此被选中的频率约为 P1 的 2 倍——CPU 时间与优先级成正比。

#### 第 3 题：`BigStride` 与 `STRIDE_MAX - STRIDE_MIN <= BigStride / 2` 的关系。

**解答：**

stride 是不断累加的量，可能溢出回绕。若使用无符号数直接比较两个 stride，当两个进程的 stride 差值在半个 `BigStride` 以内时，无符号比较结果依然正确（等价于"环形数轴上的短距离比较"）。

`BigStride` 的取值需要满足：任意时刻两个 runnable 进程的 `stride` 之差不超过 `BigStride / 2`。这是因为：

- `pass = BigStride / priority`，而 `priority >= 2`，所以每次增量 `pass <= BigStride / 2`；
- 只要任意两个 stride 的差保持在 `BigStride / 2` 以内，无符号 `a < b` 的判定就等同于"a 落后 b 不足半圈"的环形判定，从而保证"选最小 stride"的语义在溢出后依然成立。

因此实现时取 `BigStride` 为一个很大的数（本实验 `1 << 32`），并保证 `STRIDE_MAX - STRIDE_MIN <= BigStride / 2`，即可安全使用无符号比较。

#### 第 4 题：用 `PartialOrd` 和 `PartialEq` 实现 `struct Stride(u64)` 的 `partial_cmp`，需处理 `BigStride` 环绕。

**解答：**

溢出环绕下的比较不能直接用数值比较，而要处理"环形"语义。一个可行的实现如下：

```rust
const BIG_STRIDE: u64 = 1 << 32;

#[derive(Copy, Clone, Debug)]
pub struct Stride(pub u64);

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        let diff = self.0.wrapping_sub(other.0); // 环形差
        // 若 self 在 other 的"前进半圈"内，则 self < other
        if diff < BIG_STRIDE / 2 {
            Some(core::cmp::Ordering::Less)
        } else if diff > BIG_STRIDE - BIG_STRIDE / 2 {
            Some(core::cmp::Ordering::Greater)
        } else if self.0 == other.0 {
            Some(core::cmp::Ordering::Equal)
        } else {
            None // 差恰好半圈，结果不确定，保守返回 None
        }
    }
}
```

`wrapping_sub` 计算两者在环形数轴上的"前进距离"：如果 `self` 相对 `other` 前进不足半圈，说明 `self` 在"较新/较小"一侧，返回 `Less`；若超过半圈则返回 `Greater`；相等返回 `Equal`。这样即使 `stride` 溢出回绕，调度器选择"最小 stride"的判定依然正确。

---

## 三、实验验证

1. 本地构建并运行：`make -C os run BASE=2`，ch2~ch5 全部测例通过，包括 `Test spawn0 OK`、`Test wait OK`、`Test waitpid OK`、`Test set_priority OK`、`Test 04_x OK`、`Test trace OK` 等。
2. stride 公平性实测（`ch5_stride`，6 个进程 priority=5~10）：各进程输出的 `count/priority` 比值均约 3e6，`max/min` 比值远小于 1.5，调度公平。
3. 官方检查器：`make -C ci-user test CHAPTER=5` 15/15 通过。
4. 提交后 GitHub Actions 评测通过。

## 四、实验心得（可选）

本次实验最大的收获是把"进程"作为一等公民理解：TCB 从"档案袋"升级为带引用计数的"户口本"（Arc/Weak 管理父子生命周期），调度也从 FIFO 升级为带优先级的 stride 调度。实现中印象最深的是"让出前累加 stride"——必须在 `schedule` 切走之前完成，否则进程永远不会被再次公平调度；以及 `set_priority(isize::MAX)` 也要成功返回的测试要求，提醒我系统调用参数的边界情况要以测例为准。