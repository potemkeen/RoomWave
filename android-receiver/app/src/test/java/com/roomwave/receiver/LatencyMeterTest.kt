package com.roomwave.receiver

import org.junit.Assert.*
import org.junit.Test

class LatencyMeterTest {
    @Test fun accountsForClockOffsetAndPlaybackQueue() {
        val renderedAt = LatencyMeter.presentationTime(4800, 2400, 950_000_000)
        assertEquals(1_000_000_000L, renderedAt)
        assertEquals(120.0, LatencyMeter.estimateMs(980_000_000, -100_000_000, renderedAt)!!, 0.001)
    }
    @Test fun noEstimateForSilenceInvalidOrStaleMeasurements() {
        assertNull(LatencyMeter.estimateMs(0, 0, 1_000_000_000))
        assertNull(LatencyMeter.estimateMs(2_000_000_000, 0, 1_000_000_000))
        assertNull(LatencyMeter.estimateMs(1, 0, 6_000_000_000))
        val meter = LatencyMeter()
        meter.record(900_000_000, 1_000_000_000, "timestamp", 1_000_000_000)
        assertNull(meter.report(1_000_000_000).latencyMs)
        meter.sync(0, 4.0, 1_000_000_000)
        meter.record(900_000_000, 1_000_000_000, "timestamp", 1_000_000_000)
        assertEquals(100.0, meter.report(1_000_000_000).latencyMs!!, 0.001)
        assertNull(meter.report(3_000_000_000).latencyMs)
        assertEquals(4.0, meter.report(3_000_000_000).rttMs!!, 0.001)
        assertNull(meter.report(7_000_000_000).rttMs)
        assertNull(LatencyMeter().report(1_000_000_000).latencyMs)
    }
    @Test fun clockCorrectionAndMethodChangeResetSmoothing() {
        val meter = LatencyMeter()
        meter.sync(0, 2.0, 1_000_000_000)
        meter.record(900_000_000, 1_000_000_000, "queue", 1_000_000_000)
        meter.record(900_000_000, 1_050_000_000, "timestamp", 1_050_000_000)
        assertEquals(150.0, meter.report(1_050_000_000).latencyMs!!, 0.001)
        meter.sync(10_000_000, 2.0, 1_100_000_000)
        assertNull(meter.report(1_100_000_000).latencyMs)
        meter.record(1_000_000_000, 1_110_000_000, "timestamp", 1_110_000_000)
        assertEquals(100.0, meter.report(1_110_000_000).latencyMs!!, 0.001)
        meter.sync(null, null, 1_120_000_000)
        assertNull(meter.report(1_120_000_000).latencyMs)
    }
    @Test fun unwrapsAudioTrackAfterTwentyFourHours() {
        assertEquals(0xfffffff0L, LatencyMeter.unwrapPosition(0xfffffff0L, 0x100000010L))
        assertEquals(0x100000005L, LatencyMeter.unwrapPosition(5, 0x100000010L))
        assertEquals(1200L, LatencyMeter.unwrapPosition(1200, 2400))
    }
}
