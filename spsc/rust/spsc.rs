//!
//! rust spsc queue
//!

use std::sync::{Condvar, Mutex, Arc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{mem, ptr, thread};
use std::alloc;
use std::alloc::Layout;

pub enum WaitType {
    BusyWait,
    SleepWait,
}

const PAD_BYTES : usize = 7;

struct SpscQueue<T> {
    count: AtomicUsize,
    _pad1: [i64; PAD_BYTES],
    i_idx: usize,
    _pad2: [i64; PAD_BYTES],
    o_idx: usize,
    _pad3: [i64; PAD_BYTES],
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
                _pad1: [0; PAD_BYTES],
                i_idx: 0,
                _pad2: [0; PAD_BYTES],
                o_idx: 0,
                _pad3: [0; PAD_BYTES],
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

/// send/recv N times
const N : i64 = 100000000_i64;

fn recv<T>(q : &SpscQueue<T>) {
    let begin = std::time::Instant::now();
    for i in 0..N {
        let e = q.pop();
    }
    let elapse = begin.elapsed();
    println!("  recv end. {:.0} recv/ms, {:.0} ns/recv",
             N as f64 / elapse.as_millis() as f64,
             elapse.as_nanos() as f64 / N as f64);
}
fn send(q : &SpscQueue<i64>){
    for i in 0..N {
        q.push(i);
    }
}

/// test share spsc by Arc
fn test_spsc_with_arc() {
    let q1 = Arc::new(SpscQueue::<i64>::new(2 << 16, WaitType::BusyWait));
    let q2 = Arc::new(SpscQueue::<i64>::new(2 << 16, WaitType::SleepWait));

    // test q1
    println!("Arc: test spsc with busy loop...");
    let q1Clone = q1.clone();
    let thd1 = thread::spawn(move || { recv(&q1); });
    let thd2 = thread::spawn(move || { send(&q1Clone); });
    thd1.join();
    thd2.join();

    // test q2
    println!("Arc: test spsc with mutex+condition...");
    let q2Clone = q2.clone();
    let thd1 = thread::spawn(move || { recv(&q2); });
    let thd2 = thread::spawn(move || { send(&q2Clone); });
    thd1.join();
    thd2.join();
}

/// test share spsc by pointer
fn test_spsc_with_ptr() {
    let q1 = Box::new(SpscQueue::<i64>::new(2 << 16, WaitType::BusyWait));
    let q2 = Box::new(SpscQueue::<i64>::new(2 << 16, WaitType::SleepWait));
    // Box to raw pointer to be shared by threads
    let q1addr = Box::into_raw(q1) as usize;
    let q2addr = Box::into_raw(q2) as usize;

    // test q1
    println!("test spsc with busy loop...");
    let thd1 = thread::spawn(move || {
        let q = unsafe { (q1addr as *mut SpscQueue<i64>).as_mut().unwrap() };
        recv(q);
    });
    let thd2 = thread::spawn(move || {
        let q = unsafe { (q1addr as *mut SpscQueue<i64>).as_mut().unwrap() };
        send(q);
    });
    thd1.join();
    thd2.join();

    // test q2
    println!("test spsc with mutex+condition...");
    let thd1 = thread::spawn(move || {
        let q = unsafe { (q2addr as *mut SpscQueue<i64>).as_mut().unwrap() };
        recv(q);
    });
    let thd2 = thread::spawn(move || {
        let q = unsafe { (q2addr as *mut SpscQueue<i64>).as_mut().unwrap() };
        send(q);
    });
    thd1.join();
    thd2.join();

    // restore Box for cleanup
    {
        let _q1 = unsafe { Box::from_raw(q1addr as *mut SpscQueue<i64>) };
        let _q2 = unsafe { Box::from_raw(q2addr as *mut SpscQueue<i64>) };
    }
}

fn main() {
    test_spsc_with_ptr();
    test_spsc_with_arc();
}


