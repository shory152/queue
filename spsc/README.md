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

