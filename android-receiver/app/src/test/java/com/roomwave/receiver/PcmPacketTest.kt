package com.roomwave.receiver

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.*
import org.junit.Test

class PcmPacketTest {
    @Test fun monoPacketDrivesBothOutputChannelsWithoutChangingTiming() {
        val wire=ByteBuffer.allocate(544).order(ByteOrder.BIG_ENDIAN).apply {
            putInt(0x52574156); put(byteArrayOf(4,1,1,1)); putLong(123); putInt(5)
            putLong(1200); putShort(240); putShort(0); putLong(10); putLong(100); putLong(30); putLong(20)
            order(ByteOrder.LITTLE_ENDIAN)
            repeat(240) { putShort((it*100-12000).toShort()) }
        }.array()
        val decoded=PcmPlayer.decode(wire,544,123)!!
        assertEquals(100L,decoded.playNs); assertEquals(20L,decoded.readNs); assertEquals(30L,decoded.sendNs)
        val pcm=ByteBuffer.wrap(decoded.pcm).order(ByteOrder.LITTLE_ENDIAN)
        repeat(240) { val expected=(it*100-12000).toShort(); assertEquals(expected,pcm.short); assertEquals(expected,pcm.short) }
        assertNull(PcmPlayer.decode(wire.copyOf().also { it[6]=2 },544,123))
        assertNull(PcmPlayer.decode(wire,543,123))
    }
    private fun packet(frame: Long = 1200): ByteArray = ByteBuffer.allocate(1008).order(ByteOrder.BIG_ENDIAN).apply {
        putInt(0x52574156); put(byteArrayOf(4, 1, 2, 0)); putLong(123); putInt((frame / 240).toInt())
        putLong(frame); putShort(240); putShort(0); putLong(1234567890123); putLong(1235067890123); put(ByteArray(960) { 7 })
    }.array()
    @Test fun decodesSharedTimelineAndWrappedSequence() {
        val frame = (0xffffffffL + 6) * 240
        val decoded = PcmPlayer.decode(packet(frame), 1008, 123)!!
        assertEquals(frame, decoded.frame)
        assertEquals(1234567890123L, decoded.captureNs)
        assertEquals(1235067890123L, decoded.playNs)
        assertArrayEquals(ByteArray(960) { 7 }, decoded.pcm)
    }
    @Test fun rejectsWrongSessionTruncationVersionAndFrameCount() {
        assertNull(PcmPlayer.decode(packet(), 1008, 124))
        assertNull(PcmPlayer.decode(packet(), 1007, 123))
        assertNull(PcmPlayer.decode(ByteArray(10), 1008, 123))
        assertNull(PcmPlayer.decode(packet().also { it[4] = 3 }, 1008, 123))
        assertNull(PcmPlayer.decode(packet().also { it[29] = 0 }, 1008, 123))
        assertNull(PcmPlayer.decode(packet().also { it[27] = 1 }, 1008, 123))
        assertNull(PcmPlayer.decode(packet().also { for (i in 40..47) it[i] = 0 }, 1008, 123))
    }
}
