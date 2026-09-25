package com.roomwave.receiver

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.*
import org.junit.Test

class PcmFecTest {
    private fun wire(frame: Long, channels: Int=1, low: Boolean=false): ByteArray = ByteBuffer.allocate(64+(if(low) 202 else 480)*channels).order(ByteOrder.BIG_ENDIAN).apply {
        putInt(0x52574156);put(byteArrayOf(4,1,channels.toByte(),if(low) 5 else 1));putLong(42)
        putInt((frame/240).toInt());putLong(frame);putShort(240);putShort(if(low) 16000 else 0)
        putLong(10);putLong(100_000_000+frame*1_000_000_000/48000);putLong(30);putLong(20)
        while(hasRemaining()) put((position()*13+frame).toByte())
    }.array()
    private fun parity(a: ByteArray,b: ByteArray,base: Long): ByteArray = ByteBuffer.allocate(24+a.size).order(ByteOrder.BIG_ENDIAN).apply {
        putInt(0x52574658);put(1);put(0);putShort(a.size.toShort());putLong(42);putLong(base)
        for(i in a.indices) put((a[i].toInt() xor b[i].toInt()).toByte())
    }.array()
    @Test fun eitherMissingPacketIsRestoredBitExactlyInEitherArrivalOrder() {
        for(low in listOf(false,true)) for(channels in 1..2) for(missing in 0..1) for(parityFirst in listOf(false,true)) {
            val originals=listOf(wire(0,channels,low),wire(240,channels,low));val p=parity(originals[0],originals[1],0)
            val f=PcmFec(42);val good=originals[1-missing];val goodFrame=(1-missing)*240L
            val restored=if(parityFirst) {
                assertNull(f.parity(p,p.size));f.original(good,good.size,goodFrame)
            } else {
                assertNull(f.original(good,good.size,goodFrame));f.parity(p,p.size)
            }
            assertArrayEquals(originals[missing],restored)
            assertEquals(1L,f.recovered)
            assertNull(f.parity(p,p.size));assertEquals(1L,f.recovered)
            val decoded=PcmPlayer.decode(restored!!,restored.size,42)!!
            assertEquals(30L,decoded.sendNs);assertEquals(20L,decoded.readNs)
        }
    }
    @Test fun twoLossesDoNotInventAudioAndLaterPairStillRecovers() {
        val f=PcmFec(42); val a=wire(0);val b=wire(240);val p=parity(a,b,0)
        assertNull(f.parity(p,p.size));assertEquals(0L,f.recovered)
        val c=wire(480);val d=wire(720);val next=parity(c,d,480)
        assertNull(f.original(d,d.size,720));assertArrayEquals(c,f.parity(next,next.size))
    }
    @Test fun malformedOrForeignParityIsRejected() {
        val f=PcmFec(43);val a=wire(0);val b=wire(240);val p=parity(a,b,0)
        assertNull(f.parity(p,p.size));assertEquals(1L,f.invalid)
        val valid=PcmFec(42);assertNull(valid.parity(p,p.size-1))
        assertNull(valid.parity(p.copyOf().also { it[4]=2 },p.size))
        assertNull(valid.parity(p.copyOf().also { it[23]=1 },p.size))
        assertEquals(3L,valid.invalid)
    }
    @Test fun boundedCacheSurvivesWrapReorderAndNonAlignedPairs() {
        val f=PcmFec(42)
        for(n in 1..151 step 2) {
            val frame=n*240L; val a=wire(frame);val b=wire(frame+240); val p=parity(a,b,frame)
            assertNull(f.parity(p,p.size))
            assertArrayEquals(a,f.original(b,b.size,frame+240))
        }
        assertEquals(76L,f.recovered)
        val old=parity(wire(0),wire(240),0);assertNull(f.parity(old,old.size))
    }
    @Test fun retransmissionMarkerPreservesOriginalTimestamps() {
        val a=wire(0);a[7]=3
        val decoded=PcmPlayer.decode(a,a.size,42)!!
        assertEquals(30L,decoded.sendNs);assertEquals(20L,decoded.readNs)
        a[7]=2;assertNull(PcmPlayer.decode(a,a.size,42))
    }
    @Test fun burstTailIsVisibleAndWindowForgetsOldConditions() {
        val w=DelayWindow(1000)
        repeat(990) { w.add(2.0) };repeat(10) { w.add(22.0) }
        assertEquals(20.0,w.snapshot().tail,0.001)
        assertEquals(32.0,w.snapshot().jitterBudgetMs(),0.001)
        repeat(1000) { w.add(2.0) }
        assertEquals(12.0,w.snapshot().jitterBudgetMs(),0.001)
        w.add(Double.NaN);assertEquals(1000,w.snapshot().count)
    }
}
