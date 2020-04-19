use std::sync::{Mutex, Condvar, Arc};
use std::{mem, ptr, thread, time};
use std::alloc::Layout;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::str::FromStr;
use std::borrow::BorrowMut;

pub trait SenderI<T> {
    fn send(&self, e :T);
}
pub trait ReceiverI<T> {
    fn recv(&self) -> T;
}
pub struct Sender<T> {
    inner : Arc<MpmcQueue<T>>,
}
impl<T> SenderI<T> for Sender<T> {
    fn send(&self, e: T) {
        self.inner.push(e);
    }
}
pub struct Receiver<T> {
    inner: Arc<MpmcQueue<T>>,
}
impl<T> ReceiverI<T> for Receiver<T> {
    fn recv(&self) -> T {
        self.inner.pop()
    }
}

pub fn new_mpmc<T>(cap : usize) -> (Sender<T>, Receiver<T>) {
    let qs = Arc::new(MpmcQueue::<T>::new(cap));
    let qr = qs.clone();
    (Sender { inner: qs }, Receiver { inner: qr })
}

pub enum WaitType {
    BusyWait,
    SleepWait,
}

struct MpmcQueue<T> {
    count: AtomicUsize,
    _pad1: [i64; 7],
    i_idx: usize,
    _pad2: [i64; 7],
    o_idx: usize,
    _pad3: [i64; 7],
    capacity: usize,
    modulus: usize,
    buf: *const T,
    sem_room: (Mutex<()>, Condvar),
    sem_elem: (Mutex<()>, Condvar),
}

impl<T> MpmcQueue<T> {
    pub fn new(cap: usize) -> MpmcQueue<T> {
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

            MpmcQueue {
                count: AtomicUsize::new(0),
                _pad1: [0; 7],
                i_idx: 0,
                _pad2: [0; 7],
                o_idx: 0,
                _pad3: [0; 7],
                capacity: cap,
                modulus: cap - 1,
                buf: buf as *const T,
                sem_room: (Mutex::new(()), Default::default()),
                sem_elem: (Mutex::new(()), Default::default()),
            }
        }
    }

    #[inline]
    fn put_elem(&self, e : T) {
        unsafe {
            ptr::write::<T>(self.buf.offset(self.i_idx as isize) as *mut T, e);
            *(&self.i_idx as *const usize as *mut usize) = (self.i_idx + 1) & self.modulus;
        }
    }
    #[inline]
    fn get_elem(&self) -> T {
        unsafe {
            let e = ptr::read::<T>(self.buf.offset(self.o_idx as isize) as *mut T);
            *(&self.o_idx as *const usize as *mut usize) = (self.o_idx + 1) & self.modulus;
            return e;
        }
    }

    fn push(&self, e: T) {
        let mut g = self.sem_room.0.lock().unwrap();
        while self.count.load(Ordering::SeqCst) == self.capacity {
            g = self.sem_room.1.wait(g).unwrap();
        }
        self.put_elem(e);
        let c = self.count.fetch_add(1, Ordering::SeqCst);
        if c+1 < self.capacity {
            self.sem_room.1.notify_one();
        }
        drop(g);

        if c == 0 {
            let g = self.sem_elem.0.lock().unwrap();
            self.sem_elem.1.notify_one();
        }
    }
    fn pop(&self) -> T {
        let mut g = self.sem_elem.0.lock().unwrap();
        while self.count.load(Ordering::SeqCst) == 0 {
            g = self.sem_elem.1.wait(g).unwrap();
        }
        let e = self.get_elem();
        let c = self.count.fetch_sub(1, Ordering::SeqCst);
        if c-1 > 0 {
            self.sem_elem.1.notify_one();
        }
        drop(g);

        if c == self.capacity {
            let g = self.sem_room.0.lock().unwrap();
            self.sem_room.1.notify_one();
        }
        return e;
    }
}
impl<T> Drop for MpmcQueue<T> {
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
            println!("drop mpmc queue, dealloc buf {:0x}, {} bytes", self.buf as usize, buf_size);
            std::alloc::dealloc(self.buf as *mut u8, layout);
        }
    }
}
unsafe impl<T> Send for MpmcQueue<T>{}
unsafe impl<T> Sync for MpmcQueue<T>{}

impl<T> SenderI<T> for MpmcQueue<T> {
    fn send(&self, e: T) {
        self.push(e);
    }
}
impl<T> ReceiverI<T> for MpmcQueue<T> {
    fn recv(&self) -> T {
        self.pop()
    }
}

#[cfg(test)]
mod tests{
    use crate::mpmc::{SenderI, ReceiverI, MpmcQueue, WaitType, new_mpmc};
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
        let q = MpmcQueue::<i64>::new(2<<5);
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
        let (wr, rd) = new_mpmc::<i64>(2<<6);
        let t1 = thread::spawn(move || {recv(&rd);});
        let t2 = thread::spawn(move || {send(&wr);});
        t1.join();
        t2.join();
    }
}

//////////////////////// test //////////////////////////////////
struct PadI64 {
    pub Val : i64,
    _pad : [i64;7],
}
impl PadI64 {
    pub fn new(val : i64) -> PadI64 {
        PadI64{Val: val, _pad: [0;7]}
    }
    pub fn new_array(len: usize) -> Vec<PadI64> {
        let mut v = Vec::<PadI64>::with_capacity(len);
        for i in 0..len {
            v.push(PadI64::new(0));
        }
        v
    }
}

fn print_result(rs_send : &[PadI64], rs_recv : &[PadI64], interval_s : i32) {
    let mut sumTotalSend = 0i64;
    let mut sumTotalRecv = 0i64;
    let mut sumLastSend = 0i64;
    let mut sumLastRecv = 0i64;
    for v in rs_send {
        sumTotalSend += v.Val;
    }
    for v in rs_recv {
        sumTotalRecv += v.Val;
    }

    let begTotal = time::Instant::now();
    let mut begLast = begTotal;
    loop {
        begLast = time::Instant::now();
        sumLastRecv = sumTotalRecv;
        sumLastSend = sumTotalSend;
        thread::sleep(time::Duration::from_secs(interval_s as u64));
        sumTotalSend = 0;
        sumTotalRecv = 0;
        for v in rs_send {
            sumTotalSend += v.Val;
        }
        for v in rs_recv {
            sumTotalRecv += v.Val;
        }
        let deltaSend = sumTotalSend - sumLastSend;
        let deltaRecv = sumTotalRecv - sumLastRecv;
        let elapseTotal = begTotal.elapsed();
        let elapseLast = begLast.elapsed();

        print!("send: total: {:.0} send/ms, {:.0} ns/send. delta: {:.0} send/ms, {:.0} ns/send\n",
                   sumTotalSend/(elapseTotal.as_millis() as i64),
                   elapseTotal.as_nanos() as i64/sumTotalSend,
                   deltaSend/(elapseLast.as_millis() as i64),
                   elapseLast.as_nanos() as i64/deltaSend);
        print!("recv: total: {:.0} recv/ms, {:.0} ns/recv. delta: {:.0} recv/ms, {:.0} ns/recv\n",
                   sumTotalRecv/(elapseTotal.as_millis() as i64),
                   elapseTotal.as_nanos() as i64/sumTotalRecv,
                   deltaRecv/(elapseLast.as_millis() as i64),
                   elapseLast.as_nanos() as i64/deltaRecv);
    }
}

fn sendQ(q : &MpmcQueue<i64>, rs : &mut i64) {
    loop {
        q.push(1);
        *rs += 1;
    }
}

fn recvQ(q : &MpmcQueue<i64>, rs : &mut i64) {
    loop {
        let _ = q.pop();
        *rs += 1;
    }
}

//#[test]
fn main() {
    let mut nSend = 1;
    let mut nRecv = 1;
    let args : Vec<String> = std::env::args().collect();
    // println!("{:?}", args);
    if args.len() > 1 {
        if let Ok(x) = i32::from_str(&args[1]) {
            nSend = x;
        } else {
            println!("invalid args: {}", &args[1]);
            return;
        }
        if args.len() > 2 {
            if let Ok(x) = i32::from_str(&args[2]) {
                nRecv = x;
            } else {
                println!("invalid args: {}", &args[2]);
                return;
            }
        }
    }
    let capacity = (2 << 16);
    let rsSend = PadI64::new_array(nSend as usize);
    let rsRecv = PadI64::new_array(nRecv as usize);
    print!("======test rust mpmc({}): {} sender, {} receiver======\n",
             capacity, nSend, nRecv);
    let q = Arc::new(MpmcQueue::<i64>::new(capacity));

    for i in 0..nRecv {
        let sq = q.clone();
        let rs = &rsRecv[0].Val as *const i64 as usize;
        thread::spawn(move ||{
            let rs = unsafe{(rs as *const i64 as *mut i64).as_mut().unwrap()};
            recvQ(&*sq, rs);
        });
    }

    for i in 0..nSend {
        let sq = q.clone();
        let rs = &rsSend[0].Val as *const i64 as usize;
        thread::spawn(move ||{
            let rs = unsafe{(rs as *const i64 as *mut i64).as_mut().unwrap()};
            sendQ(&*sq, rs);
        });
    }

    print_result(&rsSend[..], &rsRecv[..], 10);
}
