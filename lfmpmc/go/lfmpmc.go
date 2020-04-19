// Lock-free mpmc
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

const (
	SPIN_LIMIT  uint = 6
	YIELD_LIMIT uint = 10
)

type Snoozer struct {
	count uint
}

func (self *Snoozer) spin() {
	spin_count := SPIN_LIMIT
	if self.count < spin_count {
		spin_count = self.count
	}
	for i := 0; i < (1 << spin_count); i++ {
		// notify cpu I'm spin
	}
	self.count++
}
func (self *Snoozer) snooze() {
	if self.count < SPIN_LIMIT {
		for i := 0; i < (1 << self.count); i++ {
			//
		}
	} else {
		// yield
		runtime.Gosched()
	}

	if self.count <= YIELD_LIMIT {
		self.count++
	}
}
func (self *Snoozer) completed() bool {
	return self.count > YIELD_LIMIT
}

type Slot struct {
	stamp uint64
	msg   int64
}
type Token struct {
	new_stamp uint64
	slot      *Slot
}

type Mpmc struct {
	i_idx     uint64 // queue tail
	_pad2     [7]int64
	o_idx     uint64 // queue head
	_pad3     [7]int64
	cap       uint64 //
	mark_bit  uint64 //
	one_lap   uint64 //
	buf       []Slot
	nWaitRoom int64
	nWaitElem int64
	sem_room  *sync.Cond
	sem_elem  *sync.Cond
}

func (self *Mpmc) IsFull() bool {
	head := atomic.LoadUint64(&self.o_idx)
	tail := atomic.LoadUint64(&self.i_idx)
	return head+self.one_lap == tail&^self.mark_bit
}
func (self *Mpmc) IsEmpty() bool {
	head := atomic.LoadUint64(&self.o_idx)
	tail := atomic.LoadUint64(&self.i_idx)
	return head == tail&^self.mark_bit
}
func (self *Mpmc) IsClosed() bool {
	return self.i_idx&self.mark_bit != 0
}
func (self *Mpmc) reserve_room() (bool, Token) {
	snoozer := Snoozer{}
	tail := atomic.LoadUint64(&self.i_idx)
	for {
		if tail&self.mark_bit != 0 {
			// queue is closed
			return true, Token{new_stamp: 0, slot: nil}
		}

		index := tail & (self.mark_bit - 1)
		lap := tail & ^(self.one_lap - 1)
		stamp := atomic.LoadUint64(&self.buf[index].stamp)

		if tail == stamp {
			// valid room
			next_tail := tail + 1
			if index+1 >= self.cap {
				next_tail = lap + self.one_lap
			}

			if atomic.CompareAndSwapUint64(&self.i_idx, tail, next_tail) {
				// ok, I reserved room
				return true, Token{new_stamp: tail + 1, slot: &self.buf[index]}
			} else {
				// No, the room has been reserved by others, try again.
				snoozer.spin()
				tail = atomic.LoadUint64(&self.i_idx)
			}
		} else if stamp+self.one_lap == tail+1 {
			// maybe full
			head := atomic.LoadUint64(&self.o_idx)
			if head+self.one_lap == tail {
				// queue is full
				return false, Token{}
			}
			snoozer.spin()
			tail = atomic.LoadUint64(&self.i_idx)
		} else {
			// tail may be got by other producers, try again
			snoozer.snooze()
			tail = atomic.LoadUint64(&self.i_idx)
		}
	}
	return false, Token{}
}

func (self *Mpmc) push(e int64) error {
	for {
		snoozer := Snoozer{}
		// try push several times
		for {
			if ok, token := self.reserve_room(); ok {
				if token.slot == nil {
					return fmt.Errorf("queue is closed")
				}
				token.slot.msg = e
				atomic.StoreUint64(&token.slot.stamp, token.new_stamp)
				// notify all consumers

				if atomic.LoadInt64(&self.nWaitElem) > 0 {
					self.sem_elem.L.Lock()
					self.sem_elem.Broadcast()
					self.sem_elem.L.Unlock()
				}

				return nil
			}

			// reserve room fail
			if snoozer.completed() {
				break
			} else {
				snoozer.snooze()
			}
		}

		// wait room

		if self.IsFull() {
			self.sem_room.L.Lock()
			atomic.AddInt64(&self.nWaitRoom, 1)
			self.sem_room.Wait()
			self.sem_room.L.Unlock()
			atomic.AddInt64(&self.nWaitRoom, -1)
		}

	}
}

func (self *Mpmc) reserve_elem() (bool, Token) {
	snoozer := Snoozer{}
	head := atomic.LoadUint64(&self.o_idx)
	for {
		index := head & (self.mark_bit - 1)
		lap := head & ^(self.one_lap - 1)
		stamp := atomic.LoadUint64(&self.buf[index].stamp)

		if head+1 == stamp {
			// good product
			next_head := head + 1
			if index+1 >= self.cap {
				next_head = lap + self.one_lap
			}

			if atomic.CompareAndSwapUint64(&self.o_idx, head, next_head) {
				return true, Token{new_stamp: head + self.one_lap, slot: &self.buf[index]}
			} else {
				snoozer.spin()
				head = atomic.LoadUint64(&self.o_idx)
			}
		} else if head == stamp {
			// maybe empty
			tail := atomic.LoadUint64(&self.i_idx)
			if tail&^self.mark_bit == head {
				if tail&self.mark_bit != 0 {
					// queue closed
					return true, Token{}
				}
				// empty
				return false, Token{}
			}
			snoozer.spin()
			head = atomic.LoadUint64(&self.o_idx)
		} else {
			snoozer.snooze()
			head = atomic.LoadUint64(&self.o_idx)
		}
	}
}
func (self *Mpmc) pop() int64 {
	for {
		// reserve elem several times
		snoozer := Snoozer{}
		for {
			if ok, token := self.reserve_elem(); ok {
				if token.slot == nil {
					return 0
				}
				e := token.slot.msg
				atomic.StoreUint64(&token.slot.stamp, token.new_stamp)
				if atomic.LoadInt64(&self.nWaitRoom) > 0 {
					self.sem_room.L.Lock()
					self.sem_room.Broadcast() // notify all producers
					self.sem_room.L.Unlock()
				}

				return e
			}

			if snoozer.completed() {
				break
			} else {
				snoozer.snooze()
			}
		}

		// wait elem
		if self.IsEmpty() {
			self.sem_elem.L.Lock()
			atomic.AddInt64(&self.nWaitElem, 1)
			self.sem_elem.Wait()
			self.sem_elem.L.Unlock()
			atomic.AddInt64(&self.nWaitElem, -1)
		}
	}
	return 0
}

func (self *Mpmc) close() {
	for {
		tail := atomic.LoadUint64(&self.i_idx)
		new_tail := tail | self.mark_bit
		if atomic.CompareAndSwapUint64(&self.i_idx, tail, new_tail) {
			break
		}
	}
}

func next_pow_of_two(a uint64) uint64 {
	var i uint
	for i = 0; i < 64; i++ {
		if a&(^((1 << i) - 1)) == 0 {
			break
		}
	}
	return 1 << (i)
}
func newMpmc(cap uint64) *Mpmc {
	if cap < 2 {
		panic("capacity is too small")
	}
	//cap = next_pow_of_two(cap - 1)
	q := new(Mpmc)
	q.buf = make([]Slot, cap)
	q.cap = cap
	q.mark_bit = next_pow_of_two(q.cap)
	q.one_lap = q.mark_bit << 1
	q.sem_elem = sync.NewCond(&sync.Mutex{})
	q.sem_room = sync.NewCond(&sync.Mutex{})

	for i := uint64(0); i < q.cap; i++ {
		q.buf[i].stamp = i
	}

	fmt.Printf("new mpmc, queue capacity: %x, mark: %x, lap: %x\n",
		q.cap, q.mark_bit, q.one_lap)
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

	capacity := uint64(2 << 16)
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
