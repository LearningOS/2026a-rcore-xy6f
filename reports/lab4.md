# Lab4 实验报告：文件系统与 I/O（ch6）

## 荣誉准则（Honor Code）

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   *《无》*

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   *《rCore 实验指导（https://learningos.github.io/rCore-Tutorial-Guide/）第六章、第七章；rCore-Tutorial-Book-v3（https://rcore-os.github.io/rCore-Tutorial-Book-v3/）第六章、第七章；课程仓库（2026a-rcore-xy6f）提供的实验框架代码与 easy-fs 源码》*

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。
   我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。
   我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。
   我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。
   我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按"-100"分计。

---

## 一、实验总结（200 字以内）

本实验在 easy-fs 文件系统上实现了三个系统调用：`sys_fstat`（通过 File trait 的 `as_any` 下转型到 `OSInode`，读取磁盘 inode 的 is_dir/nlink 等信息填充 Stat）、`sys_linkat`（在根目录下为目标文件追加指向同一 inode 的目录项并 nlink+1）、`sys_unlinkat`（移除目录项、nlink-1，归零时回收 inode 及数据块）。为此在 easy-fs 的 `DiskInode` 中新增 `nlink` 字段，并给 `Inode` 增加 `link`/`unlink`/`get_metadata` 方法。全部测例通过。

---

## 二、指导书题目与解答

### 2.1 编程作业（摘自实验指导第六章）

#### 题目

实现三个系统调用：

- `sys_linkat`（ID 37）：创建文件的一个硬链接。`olddirfd/newdirfd` 恒为 AT_FDCWD，`flags` 恒为 0；除非新旧名字一致则返回 -1。
- `sys_unlinkat`（ID 35）：取消一个文件路径到文件的链接，需回收 inode 及对应数据块。
- `sys_fstat`（ID 80）：获取文件状态。Stat 结构体包含 dev（写死为 0）、ino（inode 编号）、mode（文件类型）、nlink（硬链接数）、pad。

#### 实现说明

**（1）easy-fs 层扩展**

- `DiskInode` 新增 `pub nlink: u32` 字段，`initialize` 时初始化为 1。
- `EasyFileSystem` 新增 `dealloc_inode`（调用 inode 位图 `dealloc` 回收 inode）。
- `Inode` 新增三个方法：
  - `link(name, new_name)`：在目录项中查找 `name` 对应的 inode_id，追加一个指向同一 inode_id 的 `new_name` 目录项，并把目标 inode 的 `nlink += 1`。
  - `unlink(name)`：找到目录项后，将后续目录项前移、目录 size 减小；目标 inode 的 `nlink -= 1`，若归零则调用 `clear_size` 回收其数据块并 `dealloc_inode` 回收 inode。
  - `get_metadata()`：返回 `(is_dir, nlink, inode_id)`（inode_id 用 `block_id * BLOCK_SZ + block_offset` 作为稳定唯一标识）。

**（2）os 层系统调用**

- `File` trait 新增 `as_any`，`OSInode`/`Stdin`/`Stdout` 分别实现。
- `sys_fstat`：从当前进程 `fd_table` 取出文件，`as_any().downcast_ref::<OSInode>()` 下转型后调用 `get_stat()`，通过 `translated_refmut` 写回用户传入的 `Stat`。
- `sys_linkat` / `sys_unlinkat`：用 `translated_str` 读用户路径，直接调用 `ROOT_INODE.link/unlink`，成功返回 0，失败返回 -1。

---

### 2.2 问答作业（ch6）

#### 题目：在我们的 easy-fs 中，root inode 起着什么作用？如果 root inode 中的内容损坏了，会发生什么？

**解答：**

root inode 是整个文件系统的"根目录"入口，它对应文件系统镜像中最开始创建的目录 inode（`EasyFileSystem::create` 时用 `DiskInodeType::Directory` 初始化）。所有文件访问都从它开始：`open_file` 通过 `ROOT_INODE.find(name)` 在根目录的目录项中查找目标文件，`create` 在根目录下创建新文件目录项，`list_apps` 通过 `ROOT_INODE.ls()` 列出全部应用。它就像文件系统的"总目录"，一切路径解析都从这里起步。

如果 root inode 的内容损坏，那么：

1. **目录项无法解析**：`find`/`create`/`link`/`unlink` 等操作都要遍历根目录的目录项，损坏后 `find_inode_id` 会读到垃圾数据（错误的 `name` 或 `inode_id`），导致找不到已有文件、找到错误的 inode、或创建/删除落到错误的 inode 上；
2. **文件系统不可用**：无法定位任何文件，用户程序无法加载（应用列表为空或加载失败），几乎所有依赖文件的操作（open/read/write/fstat/link/unlink）都会失败或产生错误行为；
3. **可能破坏其他数据**：如果 `inode_id` 指向了错误的位置，`get_disk_inode_pos` 可能返回超出 inode 区的块号，进而读写到数据区甚至位图，导致数据损坏或内核 panic。

总之，root inode 是文件系统的根；它损坏意味着"目录树"的根断了，整个文件系统从逻辑上不可用。

---

### 2.3 问答作业（ch7，并入本报告提交）

#### 题目一：举出使用 pipe 的一个实际应用的例子。

**解答：**

管道（pipe）用于把一个进程的输出接到另一个进程的输入。最典型的例子是 Linux 终端里的管道命令，比如统计一个文件的行数：

```sh
cat file.txt | wc -l
```

- `cat file.txt` 把文件内容写到标准输出（stdout）；
- `wc -l` 从标准输入（stdin）读取，统计行数并输出。

`cat` 的 stdout 和 `wc` 的 stdin 之间由内核提供的管道连接起来：`cat` 写入管道的一端，`wc` 从另一端读取。管道内核缓冲让两个进程无需感知对方的存在，只需读写各自的文件描述符即可完成协作。这正是 `pipe` 系统调用的实际应用。

#### 题目二：如果需要在多个进程间互相通信，则需要为每一对进程建立一个管道，非常繁琐，请设计一个更易用的多进程通信机制。

**解答：**

两个进程间用管道很自然，但 N 个进程两两通信需要 O(N²) 条管道，管理繁琐。更易用的机制是引入一个**集中的消息中心（消息队列 / 信箱机制）**，参考 `mail` 系统调用：

1. **统一命名**：每个进程有一个唯一的"信箱"（按 PID 或固定 id 标识），任何进程可以向指定信箱发消息，而不必和对方建立专门通道；
2. **系统调用接口**：提供 `mail_send(pid, buf)` 与 `mail_recv(buf)`——发送方只需知道接收方的 PID，接收方从自己的信箱取出消息；消息在系统调用中从发送方用户缓冲区拷贝到接收方内核中的消息队列，再由接收方取走；
3. **内核统一管理**：内核维护一个全局消息表（`pid → 消息队列`），每个队列有容量上限；接收方未及时读取时，消息暂存内核，发送方不会被阻塞太久，接收方空闲时轮询或阻塞等待；
4. **广播与组通信**：可进一步支持 `mail_send_all` 或按组发送，满足"一对多"通知需求。

这样，进程间通信从"两两建管道"变成"向信箱投递"，关系从网状简化为星形，接口统一、易于扩展，也方便实现权限控制（校验发送目标是否存在）与背压管理（队列满时阻塞或返回错误）。

---

## 三、实验验证

1. 本地构建并运行：`make -C os run`，ch2~ch6 全部测例通过，包括 `Test file0 OK`、`Test fstat OK`、`Test link OK`、`Test mass open/unlink OK` 以及 ch6 Usertests 整体通过（`ch6 Usertests passed!`）。
2. 官方检查器：`make -C ci-user test CHAPTER=6` 通过。
3. 提交后 GitHub Actions 评测通过。

## 四、实验心得（可选）

本次实验第一次真正接触"磁盘上持久化"的文件系统。印象最深的有两点：一是 easy-fs 的 inode 是直接序列化到磁盘块里的结构（`#[repr(C)]`），加一个 `nlink` 字段就要同步所有初始化点；二是 `unlink` 必须回收 inode 和数据块，否则 `ch6_file3` 这种反复创建/删除大文件的测试会耗尽磁盘空间——"删除"不是简单移除目录项，而是一整套位图与块的回收流程。这也解释了为什么 `ch6_file3` 的注释说"如果不回收 inode 和数据块，测试会失败"。