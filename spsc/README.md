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

### test
* env: AMD Phenom(tm) II X3 710, 3core*1thread, 2.6GHz, 6G RAM.

test case              | busy loop                 | wait condition
-----------------------|---------------------------|----------------
go                     | 7612 recv/ms, 131 ns/recv | 8028 recv/ms, 124 ns/recv
rust(without Arc)      | 4307 recv/ms, 232 ns/recv | 4246 recv/ms, 236 ns/recv
rust(with Arc)         | 4326 recv/ms, 231 ns/recv | 4238 recv/ms, 236 ns/recv

