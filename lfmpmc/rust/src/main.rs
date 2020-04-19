//extern crate crossbeam_channel;

use crossbeam_channel::bounded;
use std::{time, thread};
use std::str::FromStr;

struct PadI64 {
    pub Val : i64,
    _pad : [i64;15],
}
impl PadI64 {
    pub fn new(val : i64) -> PadI64 {
        PadI64{Val: val, _pad: [0;15]}
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

fn sendQ(q : &crossbeam_channel::Sender<i64>, rs : &mut i64) {
    loop {
        q.send(1).unwrap();
        *rs += 1;
    }
}

fn recvQ(q : &crossbeam_channel::Receiver<i64>, rs : &mut i64) {
    loop {
        let _ = q.recv().unwrap();
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

    let (sdr, rvr) = crossbeam_channel::bounded::<i64>(capacity);

    for i in 0..nRecv {
        let sq = rvr.clone();
        let rs = &rsRecv[0].Val as *const i64 as usize;
        thread::spawn(move ||{
            let rs = unsafe{(rs as *const i64 as *mut i64).as_mut().unwrap()};
            recvQ(&sq, rs);
        });
    }

    for i in 0..nSend {
        let sq = sdr.clone();
        let rs = &rsSend[0].Val as *const i64 as usize;
        thread::spawn(move ||{
            let rs = unsafe{(rs as *const i64 as *mut i64).as_mut().unwrap()};
            sendQ(&sq, rs);
        });
    }

    print_result(&rsSend[..], &rsRecv[..], 10);
}

