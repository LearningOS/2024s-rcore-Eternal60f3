#### 功能

加入了一个系统调用，获取每个task的一些信息



#### 问答题

1. 发生异常`Exception::IllegalInstruction`，进入trap，在当前实现中对应的处理方式为`println!("[kernel] IllegalInstruction in application, kernel killed it.");`并exit当前应用，运行下一个应用程序

2. 入理解 [trap.S](https://github.com/LearningOS/rCore-Tutorial-Code-2024S/blob/ch3/os/src/trap/trap.S) 中两个函数 `__alltraps` 和 `__restore` 的作用

   1. 在ch2的trap.S 文件中`__restore`第一行是 `mv sp, a0`

      qemu刚启动时处于S态，此时通过调用`__restore`函数并传入第一个应用的`trap_context`在`KERNEL_STACK`的地址作为参数，来进入U态运行第一个应用，这里的`a0`就保存了传入的地址

   2. ```assembly
      ld t0, 32*8(sp) // trap_context.sstatus
      ld t1, 33*8(sp) // trap_context.sepc
      ld t2, 2*8(sp)  // trap_context.x[2] 也就是sp寄存器
      csrw sstatus, t0   // sstatus寄存器存储了S特权级多方面的信息
      csrw sepc, t1      // 记录 Trap 发生之前执行的最后一条指令的地址
      csrw sscratch, t2  // 临时中转寄存器
      ```

   3. x2即sp寄存器，此时之后还需要用到，之后通过`csrrw sp, sscratch, sp` 保存

      x4即线程指针寄存器，用不到，所以没必要保存

   4. sp指向用户栈

      sscratch指向内核栈

   5. `sret` 指令，它会完成以下功能

      + CPU 会将当前的特权级按照 `sstatus` 的 `SPP` 字段设置为 U 或者 S 
      + CPU 会跳转到 `sepc` 寄存器指向的那条指令，然后继续执行。

   6. sp指向内核栈

      sscratch指向用户栈

   7. `ecall`指令 or 发生中断或异常

   

   #### 荣誉准则

   1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：
   
      > 无
   
   2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：
   
      > rcore-v3 文档
   
   3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。
   
   4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
   
   