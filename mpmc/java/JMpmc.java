/*
 * test LinkedBlockingQueue performance
 *
 * build:
 *   javac JMpmc.java
 * usage:
 *   java JMpmc [sender_num [receiver_num]]
 * e.g.
 *   java JMpmc 2 2
 */

import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;

class PadI64 {
    long Val;
    long _pad1;
    long _pad2;
    long _pad3;
    long _pad4;
    long _pad5;
    long _pad6;
    long _pad7;

    PadI64() {
        Val = 0;
    }
    PadI64(long val){
        Val = val;
    }
    static PadI64[] getArray(int len) {
        PadI64[] ary = new PadI64[len];
        for (int i=0; i<len; i++)
            ary[i] = new PadI64();
        return ary;
    }
}

public class JMpmc {
    static void print_result(PadI64[] rsSend, PadI64[] rsRecv, int interval_s) {
        long sumLastSend = 0;
        long sumLastRecv = 0;
        long sumTotalSend = 0;
        long sumTotalRecv = 0;
        for (int i=0; i<rsSend.length; i++) {
            sumTotalSend += rsSend[i].Val;
        }
        for (int i=0; i<rsRecv.length; i++) {
            sumTotalRecv += rsRecv[i].Val;
        }
        long begTotal = System.currentTimeMillis();
        long begLast = System.currentTimeMillis();

        while (true) {
            begLast = System.currentTimeMillis();
            try{
                Thread.sleep(interval_s*1000);
                sumLastRecv = sumTotalRecv;
                sumLastSend = sumTotalSend;
                sumTotalRecv = 0;
                sumTotalSend = 0;
                for (int i=0; i<rsSend.length; i++) {
                    sumTotalSend += rsSend[i].Val;
                }
                for (int i=0; i<rsRecv.length; i++) {
                    sumTotalRecv += rsRecv[i].Val;
                }
                long deltaSend = sumTotalSend - sumLastSend;
                long deltaRecv = sumTotalRecv - sumLastRecv;
                long elapseLast = System.currentTimeMillis() - begLast;
                long elapseTotal = System.currentTimeMillis() - begTotal;

                System.out.printf("send: total: %d send/ms, %d ns/send. delta: %d send/ms, %d ns/send\n",
                        sumTotalSend/elapseTotal,
                        elapseTotal*1000000/sumTotalSend,
                        deltaSend/elapseLast,
                        elapseLast*1000000/deltaSend);
                System.out.printf("recv: total: %d recv/ms, %d ns/recv. delta: %d recv/ms, %d ns/recv\n",
                        sumTotalRecv/elapseTotal,
                        elapseTotal*1000000/sumTotalRecv,
                        deltaRecv/elapseLast,
                        elapseLast*1000000/deltaRecv);
            }catch (Exception e) {
                e.printStackTrace();
            }
        }
    }

    static void sendQ(BlockingQueue<Long> q, PadI64 rs) {
        try {
            while (true) {
                q.put(1L);
                rs.Val++;
            }
        }catch (Exception e) {
            e.printStackTrace();
        }
    }

    static void recvQ(BlockingQueue<Long> q, PadI64 rs) {
        try {
            while (true) {
                long e = q.take();
                rs.Val++;
            }
        }catch (Exception e) {
            e.printStackTrace();
        }
    }

    /**
     * Java JMpmc nSender nReceiver
     * @param args
     */
    public static void main(String[] args) {
        int nSend = 1;
        int nRecv = 1;
        if (args.length > 0) {
            try{
                nSend = Integer.parseInt(args[0]);
                if (args.length > 1) {
                    nRecv = Integer.parseInt(args[1]);
                }
            }catch(Exception e){ e.printStackTrace();}
        }
        int capacity = 2<<16;
        System.out.printf("==== java LinkedBlockingQueue(%d): %d sender, %d receiver\n",
                          capacity, nSend, nRecv);

        LinkedBlockingQueue q = new LinkedBlockingQueue<Long>(capacity);
        PadI64[] rsSend = PadI64.getArray(nSend);
        PadI64[] rsRecv = PadI64.getArray(nRecv);
        try {
            for (int i=0; i<nRecv; i++) {
                int finalI = i;
                new Thread(()->{
                    recvQ(q, rsRecv[finalI]);
                }).start();
            }

            for (int i=0; i<nSend; i++) {
                int finalI = i;
                new Thread(()->{ sendQ(q, rsSend[finalI]);}).start();
            }
        }catch (Exception e) {
            e.printStackTrace();
        }

        print_result(rsSend, rsRecv, 10);
    }
}

