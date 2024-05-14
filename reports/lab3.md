#### 实现的功能

1. 添加sys_spawn系统调用
2. 添加了 stride 调度算法



#### 问答题

tride 算法原理非常简单，但是有一个比较大的问题。例如两个 pass = 10 的进程，使用 8bit 无符号整形储存 stride， p1.stride = 255, p2.stride = 250，在 p2 执行一个时间片后，理论上下一次应该 p1 执行。

- 实际情况是轮到 p1 执行吗？为什么？
  - 答：不一定，如果`p2.prior==1`则250 + 10 = 4, 因为发生了溢出，此时仍然p2.stride最小，所以执行p2；如果`p2.prior==2`则250 + 5 = 255, 此时`p1.stride==p2.stride`相等的情况下谁先执行不确定；其他情况是p1

我们之前要求进程优先级 >= 2 其实就是为了解决这个问题。可以证明， **在不考虑溢出的情况下** , 在进程优先级全部 >= 2 的情况下，如果严格按照算法执行，那么 STRIDE_MAX – STRIDE_MIN <= BigStride / 2。

- 为什么？尝试简单说明（不要求严格证明）。
  - 答：BigStride / 2 表示的是最大能加上的 pass。每个的进程的初始 stride = 0，所以当最低优先级的进程加上最大pass后，其他进程只要stride超过前面那个最低优先级的进程，就会暂停运行了。
- 已知以上结论，**考虑溢出的情况下**，可以为 Stride 设计特别的比较器，让 BinaryHeap<Stride> 的 pop 方法能返回真正最小的 Stride。补全下列代码中的 `partial_cmp` 函数，假设两个 Stride 永远不会相等。

```rust
impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if i32::abs((self.0 as i32) - (other.0 as i32)) > BigStride / 2 {
            if self.0 < other.0 {
            Some(Ordering::Greater)
            } else {
            Some(Ordering::Less)
            }
        } else {
            Some(self.cmp(other))
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
```





#### 荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   > 无

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容

   > rcore-Tutorial-v3, rcore-Tutorial-2024

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

