### mpmc: multiple producer, multiple consumer

 sender1 -----\                           /------> receiver1
               \      +------------+     /
 sender2 -----------> | mpmc_queue |---->--------> receiver2
                /     +------------+     \
 ...           /                          \         ...
              /                            \
 senderM ----/                              \----> receiverN

### use mutex and condition
* golang: sync.Mutex + sync.Cond
* rust: sync::Mutex + sync::CondVar

### test result
** env **
* AMD Phenom(tm) II X3 710, 3core*1thread, 2.6GHz, 6G RAM.
* Linux 4.18.19-100.fc27.x86_64
* go version go1.9.2 linux/amd64
* rustc 1.37.0 (eae3437df 2019-08-13)
* OpenJDK Runtime Environment (build 1.8.0_191-b12)
* OpenJDK 64-Bit Server VM (build 25.191-b12, mixed mode)

** test case **
* java-mpmc: LinkedBlockingQueue
* go-mpmc
* go-chan: go std channel
* rust1-mpmc
* rust2-mpmc


sender | receiver | java-mpmc | go-mpmc | go-chan | rust1-mpmc | rust2-mpmc
-------|----------|-----------|---------|---------|------------|----------------
1      | 1        |
1      | 2        |
1      | 4        |
1      | 8        |
1      | 32       |
1      | 128      |
2      | 1        |
4      | 1        |
8      | 1        |
32     | 1        |
128    | 1        |
2      | 2        |
4      | 4        |
8      | 8        |
32     | 32       |
128    | 128      |

