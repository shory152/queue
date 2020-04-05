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
	BusyWait = iota
	SleepWait
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
	for atomic.LoadInt64(&self.count) == self.cap {
	}
	self.buf[self.i_idx] = e
	self.i_idx = (self.i_idx + 1) & self.mod
	atomic.AddInt64(&self.count, 1)
}
func (self *Spsc) pop_busy() int64 {
	for atomic.LoadInt64(&self.count) == 0 {
	}
	e := self.buf[self.o_idx]
	self.o_idx = (self.o_idx + 1) & self.mod
	atomic.AddInt64(&self.count, -1)
	return e
}
func (self *Spsc) push_sleep(e int64) {
	if atomic.LoadInt64(&self.count) == self.cap {
		self.sem_room.L.Lock()
		if atomic.LoadInt64(&self.count) == self.cap {
			self.sem_room.Wait()
		}
		self.sem_room.L.Unlock()
	}

	self.buf[self.i_idx] = e
	self.i_idx = (self.i_idx + 1) & self.mod
	c := atomic.AddInt64(&self.count, 1)
	if c == 1 {
		self.sem_elem.L.Lock()
		self.sem_elem.Signal()
		self.sem_elem.L.Unlock()
	}
}
func (self *Spsc) pop_sleep() int64 {
	if atomic.LoadInt64(&self.count) == 0 {
		self.sem_elem.L.Lock()
		if atomic.LoadInt64(&self.count) == 0 {
			self.sem_elem.Wait()
		}
		self.sem_elem.L.Unlock()
	}

	e := self.buf[self.o_idx]
	self.o_idx = (self.o_idx + 1) & self.mod
	c := atomic.AddInt64(&self.count, -1)
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
	fmt.Printf("new spsc, buf size: %v\n", cap)
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
		elapse := int(time.Now().Sub(beg).Nanoseconds() / 1000000)
		fmt.Printf("recv end. %v ms, %v recv/ms\n", elapse, N/elapse)
		wg.Done()
	}

	send := func(q *Spsc) {
		for i := 0; i < N; i++ {
			q.push(int64(i))
		}
	}

	wg.Add(1)
	go recv(q1)
	go send(q1)
	wg.Wait()

	wg.Add(1)
	go recv(q2)
	go send(q2)
	wg.Wait()
}
