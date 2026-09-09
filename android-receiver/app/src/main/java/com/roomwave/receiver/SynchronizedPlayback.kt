package com.roomwave.receiver

import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.roundToInt

/** Android monotonic time minus host QPC. Slew noisy network estimates without jumping playback. */
class PlaybackClock {
    private var offset: Double? = null
    private var updated = 0L
    @Synchronized fun update(value: Long?, rttMs: Double?, now: Long): Long? {
        if (value == null || rttMs == null || !rttMs.isFinite() || rttMs !in 0.0..1000.0) return null
        val old = offset
        offset = if (old == null || now - updated > 2_000_000_000L || abs(old - value) > 20_000_000) value.toDouble()
            else old + (value - old) * 0.15
        updated = now
        return offset!!.toLong()
    }
    @Synchronized fun localTime(hostNs: Long, now: Long): Long? {
        val current = offset ?: return null
        if (now - updated !in 0..2_000_000_000L) return null
        return try { Math.addExact(hostNs, current.toLong()) } catch (_: ArithmeticException) { null }
    }
}

/** Positive error means late: consume content slightly faster, keeping the hardware rate fixed. */
class SyncController {
    private var fractional = 0.0
    fun outputFrames(errorNs: Long): Int {
        val ratio = (1.0 - errorNs / 1_000_000_000.0 * 0.25).coerceIn(0.995, 1.005)
        val exact = 240 * ratio + fractional
        val frames = floor(exact).toInt()
        fractional = exact - frames
        return frames
    }
    fun reset() { fractional = 0.0 }
    companion object {
        const val HARD_LIMIT_NS = 20_000_000L
        fun targetTime(anchorFrame: Long, anchorNs: Long, frame: Long): Long =
            anchorNs + (frame - anchorFrame) * 1_000_000_000L / 48000
        /** Stereo linear resampling. Small +/-0.5% corrections avoid repeated packet drops. */
        fun resample(pcm: ByteArray, outputFrames: Int): ByteArray {
            require(pcm.size == 960 && outputFrames in 238..242)
            if (outputFrames == 240) return pcm
            val result = ByteArray(outputFrames * 4)
            fun sample(frame: Int, channel: Int): Int {
                val i = frame * 4 + channel * 2
                return ((pcm[i].toInt() and 255) or (pcm[i + 1].toInt() shl 8)).toShort().toInt()
            }
            for (i in 0 until outputFrames) {
                val position = i * 239.0 / (outputFrames - 1)
                val left = floor(position).toInt(); val right = minOf(239, left + 1)
                val fraction = position - left
                for (channel in 0..1) {
                    val value = (sample(left, channel) * (1.0 - fraction) + sample(right, channel) * fraction).roundToInt().coerceIn(-32768, 32767)
                    val index = i * 4 + channel * 2
                    result[index] = value.toByte(); result[index + 1] = (value shr 8).toByte()
                }
            }
            return result
        }
    }
}

data class SyncReport(val status: String = "warming", val errorMs: Double? = null)
