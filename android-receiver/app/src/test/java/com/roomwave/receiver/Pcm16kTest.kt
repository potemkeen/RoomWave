package com.roomwave.receiver

import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.*
import org.junit.Assert.*
import org.junit.Test

class Pcm16kTest {
    private fun packet(frame:Long=0,channels:Int=1):ByteArray=ByteBuffer.allocate(64+202*channels).order(ByteOrder.BIG_ENDIAN).apply {
        putInt(0x52574156);put(byteArrayOf(4,1,channels.toByte(),5));putLong(42)
        putInt((frame/240).toInt());putLong(frame);putShort(240);putShort(16000)
        putLong(10);putLong(100_000_000);putLong(30);putLong(20)
        order(ByteOrder.LITTLE_ENDIAN)
        for(n in -21 until 80) for(c in 0 until channels) putShort((10000*sin(2*PI*1000*(n*3+frame)/48000)*(if(c==0) 1 else -1)).roundToInt().toShort())
    }.array()
    @Test fun expandsTo48kWithoutChangingDurationAndDuplicatesMono() {
        for(channels in 1..2) {
            val wire=packet(channels=channels);val p=PcmPlayer.decode(wire,wire.size,42)!!
            assertEquals(16000,p.sampleRate);assertEquals(960,p.pcm.size);assertEquals(100_000_000,p.playNs)
            val pcm=ByteBuffer.wrap(p.pcm).order(ByteOrder.LITTLE_ENDIAN)
            for(n in 0 until 240) {
                val l=pcm.short.toInt();val r=pcm.short.toInt()
                assertEquals(if(channels==1) l else -l,r)
                val expected=10000*sin(2*PI*1000*(n-31)/48000)
                assertTrue("sample $n: $l / $expected",abs(l-expected)<100)
            }
        }
    }
    @Test fun packetHistoryMakesLossAndReorderIndependent() {
        val b=packet(240); val first=PcmPlayer.decode(b,b.size,42)!!
        val a=packet();PcmPlayer.decode(a,a.size,42)
        assertArrayEquals(first.pcm,PcmPlayer.decode(b,b.size,42)!!.pcm)
    }
    @Test fun rejectsWrongRateLengthAndSupportsMarkedRepair() {
        val a=packet();assertNull(PcmPlayer.decode(a,a.size-1,42))
        assertNull(PcmPlayer.decode(a.copyOf().also {it[30]=0;it[31]=0},a.size,42))
        assertNull(PcmPlayer.decode(a.copyOf().also {it[7]=1},a.size,42))
        a[7]=7;assertEquals(16000,PcmPlayer.decode(a,a.size,42)!!.sampleRate)
    }
}
