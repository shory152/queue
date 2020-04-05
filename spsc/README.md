### spsc: single producer, single consumer

thread1(goroutine1) ---send----> spsc_queue ---recv---> thread2(goroutine2)

### queue wait mode:
* busy loop: query room or product with busy loop
* sleep wait: wait for room or product until be notify.

### use atomic CAS
* load
* store
* AtomicAdd/fetch_add

### use mutex and condition
* golang: sync.Mutex + sync.Cond
* rust: sync::Mutex + sync::CondVar

### test result
** env **
* AMD Phenom(tm) II X3 710, 3core*1thread, 2.6GHz, 6G RAM.
* Linux 4.18.19-100.fc27.x86_64
* go version go1.9.2 linux/amd64
* rustc 1.37.0 (eae3437df 2019-08-13)

** test case **
* go
* rust1_1(SpscQueue without Arc).
* rust1_2(SpscQueue with Arc).
* rust2_1(Sender/Receiver wrapper), refer to spsc2.rs.

test case              | busy loop                 | wait condition
-----------------------|---------------------------|----------------
go                     | 7612 recv/ms, 131 ns/recv | 8028 recv/ms, 124 ns/recv
rust1_1                | 4307 recv/ms, 232 ns/recv | 4246 recv/ms, 236 ns/recv
rust1_2                | 4326 recv/ms, 231 ns/recv | 4238 recv/ms, 236 ns/recv
rust2_1                | 3882 recv/ms, 258 ns/recv | 3810 recv/ms, 262 ns/recv

