use std::sync::{Mutex, Condvar, Arc};
use std::{mem, ptr};
use std::alloc::Layout;
use std::sync::atomic::{AtomicUsize, Ordering};

pub trait SenderI<T> {
    fn send(&self, e :T);
}
pub trait ReceiverI<T> {
    fn recv(&self) -> T;
}
pub struct Sender<T> {
    inner : Arc<SpscQueue<T>>,
}
impl<T> SenderI<T> for Sender<T> {
    fn send(&self, e: T) {
        self.inner.push(e);
    }
}
pub struct Receiver<T> {
    inner: Arc<SpscQueue<T>>,
}
impl<T> ReceiverI<T> for Receiver<T> {
    fn recv(&self) -> T {
        self.inner.pop()
    }
}

pub fn new_spsc<T>(cap : usize, wait_mode : WaitType) -> (Sender<T>, Receiver<T>) {
    let qs = Arc::new(SpscQueue::<T>::new(cap, wait_mode));
    let qr = qs.clone();
    (Sender { inner: qs }, Receiver { inner: qr })
}

pub enum WaitType {
    BusyWait,
    SleepWait,
}

struct SpscQueue<T> {
    count: AtomicUsize,
    _pad1: [i64; 7],
    i_idx: usize,
    _pad2: [i64; 7],
    o_idx: usize,
    _pad3: [i64; 7],
    capacity: usize,
    mode: usize,
    wait_mode: WaitType,
    buf: *const T,
    sem_room: (Mutex<()>, Condvar),
    sem_elem: (Mutex<()>, Condvar),
}

impl<T> SpscQueue<T> {
    pub fn new(cap: usize, wait_mode: WaitType) -> SpscQueue<T> {
        assert!(mem::size_of::<T>() != 0, "not support ZST");
        assert!(cap >= 1, "capacity too small");

        unsafe {
            let buf_size = std::mem::size_of::<T>() * cap;
            let align = mem::align_of::<T>();
            let layout = Layout::from_size_align(buf_size, align).unwrap();

            let buf = std::alloc::alloc(layout);
            if buf.is_null() {
                panic!("Out of memory")
            }
            println!("new spsc, queue capaciry: {}, buffer size: {} bytes, alloc buf {:0x}",
                     cap, buf_size, buf as usize);

            SpscQueue {
                count: AtomicUsize::new(0),
                _pad1: [0; 7],
                i_idx: 0,
                _pad2: [0; 7],
                o_idx: 0,
                _pad3: [0; 7],
                capacity: cap,
                mode: cap - 1,
                buf: buf as *const T,
                wait_mode,
                sem_room: (Mutex::new(()), Default::default()),
                sem_elem: (Mutex::new(()), Default::default()),
            }
        }
    }

    fn put_elem(&self, e : T) {
        unsafe {
            ptr::write::<T>(self.buf.offset(self.i_idx as isize) as *mut T, e);
            *(&self.i_idx as *const usize as *mut usize) = (self.i_idx + 1) & self.mode;
        }
    }
    fn get_elem(&self) -> T {
        unsafe {
            let e = ptr::read::<T>(self.buf.offset(self.o_idx as isize) as *mut T);
            *(&self.o_idx as *const usize as *mut usize) = (self.o_idx + 1) & self.mode;
            return e;
        }
    }

    fn push_busy(&self, e: T) {
        while self.count.load(Ordering::SeqCst) == self.capacity {}
        self.put_elem(e);
        self.count.fetch_add(1, Ordering::SeqCst);
    }
    fn pop_busy(&self) -> T {
        while self.count.load(Ordering::SeqCst) == 0 {}
        let e = self.get_elem();
        self.count.fetch_sub(1, Ordering::SeqCst);
        return e;
    }

    fn push_sleep(&self, e: T) {
        if self.count.load(Ordering::SeqCst) == self.capacity {
            let g = self.sem_room.0.lock().unwrap();
            if self.count.load(Ordering::SeqCst) == self.capacity {
                self.sem_room.1.wait(g).unwrap();
            }
        }
        self.put_elem(e);
        let c = self.count.fetch_add(1, Ordering::SeqCst);
        if c == 0 {
            let g = self.sem_elem.0.lock().unwrap();
            self.sem_elem.1.notify_one();
        }
    }
    fn pop_sleep(&self) -> T {
        if self.count.load(Ordering::SeqCst) == 0 {
            let g = self.sem_elem.0.lock().unwrap();
            if self.count.load(Ordering::SeqCst) == 0 {
                self.sem_elem.1.wait(g);
            }
        }
        let e = self.get_elem();
        let c = self.count.fetch_sub(1, Ordering::SeqCst);
        if c+1 == self.capacity {
            let g = self.sem_room.0.lock().unwrap();
            self.sem_room.1.notify_one();
        }
        return e;
    }

    #[inline]
    pub fn push(&self, e : T) {
        match  self.wait_mode {
            WaitType::BusyWait => self.push_busy(e),
            WaitType::SleepWait => self.push_sleep(e),
        };
    }

    #[inline]
    pub fn pop(&self) -> T {
        match  self.wait_mode {
            WaitType::BusyWait => self.pop_busy(),
            WaitType::SleepWait => self.pop_sleep(),
        }
    }
}
impl<T> Drop for SpscQueue<T> {
    fn drop(&mut self) {
        // remove all elements
        while self.count.load(Ordering::SeqCst) > 0 {
            let _ = self.pop();
        }

        // free buffer
        unsafe {
            let buf_size = mem::size_of::<T>() * self.capacity;
            let align = mem::align_of::<T>();
            let layout = Layout::from_size_align(buf_size, align).unwrap();
            println!("drop spsc queue, dealloc buf {:0x}, {} bytes", self.buf as usize, buf_size);
            std::alloc::dealloc(self.buf as *mut u8, layout);
        }
    }
}
unsafe impl<T> Send for SpscQueue<T>{}
unsafe impl<T> Sync for SpscQueue<T>{}

impl<T> SenderI<T> for SpscQueue<T> {
    fn send(&self, e: T) {
        self.push(e);
    }
}
impl<T> ReceiverI<T> for SpscQueue<T> {
    fn recv(&self) -> T {
        self.pop()
    }
}

#[cfg(test)]
mod tests{
    use crate::spsc2::{SenderI, ReceiverI, SpscQueue, WaitType, new_spsc};
    use std::sync::Arc;
    use std::thread;

    fn send(w : &dyn SenderI<i64>) {
        for i in 0..10 {
            w.send(i as i64);
            println!("send {}", i);
        }
    }
    fn recv(r : &dyn ReceiverI<i64>) {
        for i in 0..10 {
            let e = r.recv();
            println!("--recv {}", e);
        }
    }

    #[test]
    fn test1() {
        let q = SpscQueue::<i64>::new(2<<5, WaitType::SleepWait);
        send(&q);
        recv(&q);
        // drop(q);

        let q = Arc::new(q);
        send(&*q);
        recv(&*q);

        let qc = q.clone();
        let t = thread::spawn(move ||{ send(&*qc);});
        recv(&*q);
        t.join();
    }

    #[test]
    fn test2(){
        let (wr, rd) = new_spsc::<i64>(2<<6, WaitType::SleepWait);
        let t1 = thread::spawn(move || {recv(&rd);});
        let t2 = thread::spawn(move || {send(&wr);});
        t1.join();
        t2.join();
    }
}

fn recv<T>(q : &dyn ReceiverI<T>, n: i64) {
    let begin = std::time::Instant::now();
    for i in 0..n {
        let e = q.recv();
    }
    let elapse = begin.elapsed();
    println!("  recv end. {:.0} recv/ms, {:.0} ns/recv",
             N as f64 / elapse.as_millis() as f64,
             elapse.as_nanos() as f64 / N as f64);
}
fn send(q : &dyn SenderI<i64>, n: i64){
    for i in 0..n {
        q.send(i);
    }
}

/// send/recv N times
const N : i64 = 100000000_i64;

fn main() {
    println!("test spsc with busy loop...");
    let (wr, rd) = new_spsc::<i64>(2<<16, WaitType::BusyWait);
    let t1 = std::thread::spawn(move ||{recv(&rd, N);});
    let t2 = std::thread::spawn(move ||{send(&wr, N);});
    t1.join();
    t2.join();

    println!("test spsc with mutex+condition...");
    let (wr, rd) = new_spsc::<i64>(2<<16, WaitType::SleepWait);
    let t1 = std::thread::spawn(move ||{recv(&rd, N);});
    let t2 = std::thread::spawn(move ||{send(&wr, N);});
    t1.join();
    t2.join();

    println!("Sender/Receiver 2: test spsc with busy loop...");
    let (wr, rd) = new_spsc::<i64>(2<<16, WaitType::BusyWait);
    let t1 = std::thread::spawn(move ||{
        let begin = std::time::Instant::now();
        for i in 0..N {
            let e = rd.recv();
        }
        let elapse = begin.elapsed();
        println!("  recv end. {:.0} recv/ms, {:.0} ns/recv",
                 N as f64 / elapse.as_millis() as f64,
                 elapse.as_nanos() as f64 / N as f64);
    });
    let t2 = std::thread::spawn(move ||{
        for i in 0..N {
            wr.send(i);
        }
    });
    t1.join();
    t2.join();

    println!("Sender/Receiver 2: test spsc with wait condition...");
    let (wr, rd) = new_spsc::<i64>(2<<16, WaitType::SleepWait);
    let t1 = std::thread::spawn(move ||{
        let begin = std::time::Instant::now();
        for i in 0..N {
            let e = rd.recv();
        }
        let elapse = begin.elapsed();
        println!("  recv end. {:.0} recv/ms, {:.0} ns/recv",
                 N as f64 / elapse.as_millis() as f64,
                 elapse.as_nanos() as f64 / N as f64);
    });
    let t2 = std::thread::spawn(move ||{
        for i in 0..N {
            wr.send(i);
        }
    });
    t1.join();
    t2.join();
}
