package com.roomwave.receiver

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Network-thread-only, bounded cache. Repairs contain the entire original wire packet. */
class PcmFec(private val session: Long) {
    private data class Original(val frame: Long, val bytes: ByteArray)
    private data class Repair(val base: Long, val bytes: ByteArray)
    private val originals = arrayOfNulls<Original>(64)
    private val repairs = arrayOfNulls<Repair>(64)
    private var highest = -240L
    var invalid = 0L; private set
    var recovered = 0L; private set
    private fun index(frame: Long) = ((frame / 240) % 64).toInt()

    fun original(bytes: ByteArray, length: Int, frame: Long): ByteArray? {
        if (frame < highest - 63*240) return null
        highest = maxOf(highest, frame)
        originals[index(frame)] = Original(frame, bytes.copyOf(length))
        // A pair can start at any frame after a discontinuity or channel change.
        return recover(frame) ?: if (frame >= 240) recover(frame-240) else null
    }

    fun parity(bytes: ByteArray, length: Int): ByteArray? {
        if ((length!=290 && length!=492 && length!=568 && length!=1048) || bytes.size < length) { invalid++; return null }
        val b = ByteBuffer.wrap(bytes,0,length).order(ByteOrder.BIG_ENDIAN)
        if (b.int != 0x52574658 || b.get().toInt()!=1 || b.get().toInt()!=0) { invalid++; return null }
        val size = b.short.toInt() and 0xffff
        val id = b.long; val base = b.long
        if (size+24!=length || id!=session || base<0 || base%240!=0L || base>Long.MAX_VALUE-240) { invalid++; return null }
        if (base < highest-63*240) return null
        repairs[index(base)] = Repair(base,bytes.copyOfRange(24,length))
        return recover(base)
    }

    private fun recover(base: Long): ByteArray? {
        val slot=index(base)
        val repair=repairs[slot]?.takeIf { it.base==base } ?: return null
        val a=originals[index(base)]?.takeIf { it.frame==base }
        val b=originals[index(base+240)]?.takeIf { it.frame==base+240 }
        if(a!=null && b!=null) { repairs[slot]=null; return null }
        val present=a?:b?:return null
        if(present.bytes.size!=repair.bytes.size) { repairs[slot]=null; invalid++; return null }
        val wire=ByteArray(repair.bytes.size) { (repair.bytes[it].toInt() xor present.bytes[it].toInt()).toByte() }
        repairs[slot]=null
        val decoded=PcmPlayer.decode(wire,wire.size,session)
        val expected=if(a==null) base else base+240
        if(decoded==null || decoded.frame!=expected) { invalid++; return null }
        originals[index(expected)]=Original(expected,wire)
        recovered++
        return wire
    }
}
