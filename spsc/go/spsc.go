package main

import (
	"fmt"
	"runtime"
	"sync"
	"sync/atomic"
	"time"
)

type WaitType int

const (
	BusyWait  = iota // busy loop on CAS
	SleepWait        // wait on condition
)

type Spsc struct {
	count     int64
	_pad1     [7]int64
	i_idx     int64
	_pad2     [7]int64
	o_idx     int64
	_pad3     [7]int64
	cap       int64
	mod       int64 // must be power of two
	buf       []int64
	sem_room  *sync.Cond
	sem_elem  *sync.Cond
	wait_mode WaitType
}

func (self *Spsc) push_busy(e int64) {
	// wait for room
	for atomic.LoadInt64(&self.count) == self.cap {
	}
	// put element
	self.buf[self.i_idx] = e
	self.i_idx = (self.i_idx + 1) & self.mod
	// add elem count
	atomic.AddInt64(&self.count, 1)
}

func (self *Spsc) pop_busy() int64 {
	// wait for element
	for atomic.LoadInt64(&self.count) == 0 {
	}
	// consume element
	e := self.buf[self.o_idx]
	self.o_idx = (self.o_idx + 1) & self.mod
	// sub elem count
	atomic.AddInt64(&self.count, -1)
	return e
}

func (self *Spsc) push_sleep(e int64) {
	// wait for room
	if atomic.LoadInt64(&self.count) == self.cap {
		self.sem_room.L.Lock()
		if atomic.LoadInt64(&self.count) == self.cap {
			self.sem_room.Wait()
		}
		self.sem_room.L.Unlock()
	}
	// put element
	self.buf[self.i_idx] = e
	self.i_idx = (self.i_idx + 1) & self.mod
	// add elem count
	c := atomic.AddInt64(&self.count, 1)
	// notify consumer
	if c == 1 {
		self.sem_elem.L.Lock()
		self.sem_elem.Signal()
		self.sem_elem.L.Unlock()
	}
}

func (self *Spsc) pop_sleep() int64 {
	// wait for element
	if atomic.LoadInt64(&self.count) == 0 {
		self.sem_elem.L.Lock()
		if atomic.LoadInt64(&self.count) == 0 {
			self.sem_elem.Wait()
		}
		self.sem_elem.L.Unlock()
	}
	// consume element
	e := self.buf[self.o_idx]
	self.o_idx = (self.o_idx + 1) & self.mod
	// sub elem count
	c := atomic.AddInt64(&self.count, -1)
	// notify producer
	if c+1 == self.cap {
		self.sem_room.L.Lock()
		self.sem_room.Signal()
		self.sem_room.L.Unlock()
	}
	return e
}

func (self *Spsc) push(e int64) {
	if self.wait_mode == BusyWait {
		self.push_busy(e)
	} else {
		self.push_sleep(e)
	}
	return
}

func (self *Spsc) pop() int64 {
	if self.wait_mode == BusyWait {
		return self.pop_busy()
	} else {
		return self.pop_sleep()
	}
}

func newSpsc(cap int64, waitMode WaitType) *Spsc {
	q := new(Spsc)
	q.buf = make([]int64, cap)
	q.cap = cap
	q.mod = cap - 1
	q.sem_elem = sync.NewCond(&sync.Mutex{})
	q.sem_room = sync.NewCond(&sync.Mutex{})
	q.wait_mode = waitMode
	fmt.Printf("new spsc, queue capacity: %v, buf size: %v bytes\n", cap, cap*8)
	return q
}

func main() {
	runtime.GOMAXPROCS(runtime.NumCPU())

	q1 := newSpsc(2<<16, BusyWait)
	q2 := newSpsc(2<<16, SleepWait)

	N := (100000000)
	wg := sync.WaitGroup{}

	recv := func(q *Spsc) {
		beg := time.Now()
		for i := 0; i < N; i++ {
			e := q.pop()
			_ = e
		}
		elapse := time.Now().Sub(beg)
		elapse_ms := elapse.Nanoseconds() / 1000000
		fmt.Printf("  recv end. %v recv/ms, %v ns/recv\n",
			int64(N)/elapse_ms, elapse.Nanoseconds()/int64(N))
		wg.Done()
	}

	send := func(q *Spsc) {
		for i := 0; i < N; i++ {
			q.push(int64(i))
		}
	}

	fmt.Printf("test spsc with busy loop...\n")
	wg.Add(1)
	go recv(q1)
	go send(q1)
	wg.Wait()

	fmt.Printf("test spsc with mutex+condition...\n")
	wg.Add(1)
	go recv(q2)
	go send(q2)
	wg.Wait()
}

