package com.roomwave.receiver

import org.junit.Assert.*
import org.junit.Test
import kotlin.math.abs

class SynchronizedPlaybackTest {
    @Test fun clocksWithDifferentOriginsMapToOneDeadlineAndExpire() {
        val a = PlaybackClock(); val b = PlaybackClock()
        a.update(-100_000_000_000, 6.0, 1_000_000_000)
        b.update(300_000_000_000, 8.0, 1_000_000_000)
        assertEquals(23_500_000_000L, a.localTime(123_500_000_000, 1_000_000_000))
        assertEquals(423_500_000_000L, b.localTime(123_500_000_000, 1_000_000_000))
        assertNull(a.localTime(123_500_000_000, 4_000_000_000))
        assertNull(PlaybackClock().localTime(1, 1))
        a.update(-99_998_000_000, 6.0, 1_500_000_000)
        assertEquals(23_500_300_000L, a.localTime(123_500_000_000, 1_500_000_000))
    }
    @Test fun lateJoinAndMissingPacketsKeepAbsoluteTimeline() {
        val frame = 240L * 10000
        val time = 60_000_000_000L
        assertEquals(time + 5_000_000, SyncController.targetTime(frame, time, frame + 240))
        assertEquals(time + 100_000_000, SyncController.targetTime(frame, time, frame + 20 * 240))
        assertEquals(time, SyncController.targetTime(frame + 20 * 240, time + 100_000_000, frame))
    }
    @Test fun resamplingPreservesStereoConstantSignalAndEndpoints() {
        val pcm = ByteArray(960)
        for (frame in 0..239) {
            pcm[frame * 4] = 0x34; pcm[frame * 4 + 1] = 0x12
            pcm[frame * 4 + 2] = 0xcc.toByte(); pcm[frame * 4 + 3] = 0xed.toByte()
        }
        for (size in 238..242) {
            val result = SyncController.resample(pcm, size)
            assertEquals(size * 4, result.size)
            for (frame in 0 until size) assertArrayEquals(pcm.copyOfRange(0, 4), result.copyOfRange(frame * 4, frame * 4 + 4))
        }
    }
    @Test fun fractionalCorrectionDoesNotRoundAwaySmallClockDrift() {
        val controller = SyncController()
        var total = 0
        repeat(10000) { total += controller.outputFrames(400_000) } // 100 ppm.
        assertEquals(2_399_760.0, total.toDouble(), 1.0)
    }
    @Test fun twoReceiversConvergeDespiteDifferentLatencyAndOppositeClockDrift() {
        fun simulate(initialErrorMs: Double, driftPpm: Double): Double {
            val controller = SyncController()
            var nextNs = 500_000_000.0 + initialErrorMs * 1_000_000
            val rate = 48000.0 * (1.0 + driftPpm / 1_000_000)
            var peakAfterSettling = 0.0
            for (packet in 0..120000) { // Ten virtual minutes, no wall-clock waits.
                val target = 500_000_000L + packet * 5_000_000L
                while (nextNs - target < -SyncController.HARD_LIMIT_NS) nextNs += 240 * 1_000_000_000.0 / rate
                val error = (nextNs - target).toLong()
                if (error > SyncController.HARD_LIMIT_NS) { controller.reset(); continue }
                val frames = controller.outputFrames(error)
                if (packet > 10000) peakAfterSettling = maxOf(peakAfterSettling, abs(error.toDouble()))
                nextNs += frames * 1_000_000_000.0 / rate
            }
            assertTrue("phase bound $peakAfterSettling ns", peakAfterSettling < 1_500_000)
            return nextNs - (500_000_000L + 120001L * 5_000_000L)
        }
        val a = simulate(-120.0, 200.0)
        val b = simulate(170.0, -250.0)
        assertTrue("receivers differ by ${a - b} ns", abs(a - b) < 3_000_000)
    }
}
