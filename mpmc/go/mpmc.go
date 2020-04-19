//
// test go mpmc performance
// build:
//   go build mpmc.go
// usage:
//   mpmc [sender_num [receiver_num]]
// e.g.
//   mpmc 1 1
//

package main

import (
	"fmt"
	"os"
	"runtime"
	"strconv"
	"sync"
	"sync/atomic"
	"time"
)

const PAD_BYTES = 7

type Mpmc struct {
	count    int64
	_pad1    [PAD_BYTES]int64
	i_idx    int64
	_pad2    [PAD_BYTES]int64
	o_idx    int64
	_pad3    [PAD_BYTES]int64
	cap      int64 // must be power of two
	mod      int64 // cap-1
	buf      []int64
	sem_room *sync.Cond
	sem_elem *sync.Cond
}

func (self *Mpmc) push(e int64) {
	// wait for room
	self.sem_room.L.Lock()
	for atomic.LoadInt64(&self.count) == self.cap { // need atomic load ???
		self.sem_room.Wait()
	}
	// put element
	self.buf[self.i_idx] = e
	self.i_idx = (self.i_idx + 1) & self.mod
	// add elem count
	c := atomic.AddInt64(&self.count, 1)
	if c < self.cap {
		// more room, notify other producers
		self.sem_room.Signal()
	}
	self.sem_room.L.Unlock()

	// notify consumer
	if c == 1 {
		self.sem_elem.L.Lock()
		self.sem_elem.Signal()
		self.sem_elem.L.Unlock()
	}
}

func (self *Mpmc) pop() int64 {
	// wait for element
	self.sem_elem.L.Lock()
	for atomic.LoadInt64(&self.count) == 0 {
		self.sem_elem.Wait()
	}
	// consume element
	e := self.buf[self.o_idx]
	self.o_idx = (self.o_idx + 1) & self.mod
	// sub elem count
	c := atomic.AddInt64(&self.count, -1)
	if c > 0 {
		// more product, notify other consumers
		self.sem_elem.Signal()
	}
	self.sem_elem.L.Unlock()

	// notify producer
	if c+1 == self.cap {
		self.sem_room.L.Lock()
		self.sem_room.Signal()
		self.sem_room.L.Unlock()
	}
	return e
}

func newMpmc(cap int64) *Mpmc {
	if cap < 2 {
		panic("capacity is too small")
	}
	q := new(Mpmc)
	q.buf = make([]int64, cap)
	q.cap = cap
	q.mod = cap - 1
	q.sem_elem = sync.NewCond(&sync.Mutex{})
	q.sem_room = sync.NewCond(&sync.Mutex{})
	//q.wait_mode = waitMode
	fmt.Printf("new mpmc, queue capacity: %v, buf size: %v bytes\n", cap, cap*8)
	return q
}

type PadI64 struct {
	Val int64
	pad [7]int64
}

func PrintResult(sendResult []PadI64, recvResult []PadI64, interval time.Duration) {
	sumTotalSend := int64(0)
	sumTotalRecv := int64(0)
	for _, v := range sendResult {
		sumTotalSend += v.Val
	}
	for _, v := range recvResult {
		sumTotalRecv += v.Val
	}
	sumLastSend := sumTotalSend
	sumLastRecv := sumTotalRecv
	begTotal := time.Now()
	begLast := time.Now()

	for {
		time.Sleep(interval)
		sumTotalSend = 0
		sumTotalRecv = 0
		for _, v := range sendResult {
			sumTotalSend += v.Val
		}
		for _, v := range recvResult {
			sumTotalRecv += v.Val
		}
		elapseLast := time.Now().Sub(begLast)
		elapseTotal := time.Now().Sub(begTotal)
		deltaSend := sumTotalSend - sumLastSend
		deltaRecv := sumTotalRecv - sumLastRecv
		fmt.Printf("send: total: %d send/ms, %d ns/send. delta: %d send/ms, %d ns/send\n",
			sumTotalSend/(elapseTotal.Nanoseconds()/1000000),
			elapseTotal.Nanoseconds()/sumTotalSend,
			deltaSend/(elapseLast.Nanoseconds()/1000000),
			elapseLast.Nanoseconds()/deltaSend)
		fmt.Printf("recv: total: %d recv/ms, %d ns/recv. delta: %d recv/ms, %d ns/recv\n",
			sumTotalRecv/(elapseTotal.Nanoseconds()/1000000),
			elapseTotal.Nanoseconds()/sumTotalRecv,
			deltaRecv/(elapseLast.Nanoseconds()/1000000),
			elapseLast.Nanoseconds()/deltaRecv)

		sumLastSend = sumTotalSend
		sumLastRecv = sumTotalRecv
		begLast = time.Now()
	}
}
func send(q *Mpmc, result *PadI64) {
	var e int64 = 123
	for {
		q.push(e)
		result.Val++
	}
}
func recv(q *Mpmc, result *PadI64) {
	for {
		_ = q.pop()
		result.Val++
	}
}

// mpmc nSender nReceiver
func main() {
	runtime.GOMAXPROCS(runtime.NumCPU())

	var nSend int = 1
	var nRecv int = 1
	if len(os.Args) > 1 {
		if v, err := strconv.ParseInt(os.Args[1], 10, 32); err == nil {
			nSend = int(v)
		} else {
			fmt.Println(err)
		}
		if len(os.Args) > 2 {
			if v, err := strconv.ParseInt(os.Args[2], 10, 32); err == nil {
				nRecv = int(v)
			} else {
				fmt.Println(err)
			}
		}
	}

	rsSend := make([]PadI64, nSend)
	rsRecv := make([]PadI64, nRecv)

	capacity := int64(2 << 16)
	q := newMpmc(capacity)

	fmt.Printf("======test go mpmc(%v): %v sender, %v receiver======\n",
		capacity, nSend, nRecv)
	for i := 0; i < nRecv; i++ {
		go recv(q, &rsRecv[i])
	}

	for i := 0; i < nSend; i++ {
		go send(q, &rsSend[i])
	}

	PrintResult(rsSend, rsRecv, 10*time.Second)
}
