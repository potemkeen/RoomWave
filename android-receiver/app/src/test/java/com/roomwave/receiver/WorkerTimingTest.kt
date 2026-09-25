package com.roomwave.receiver

import org.junit.Assert.*
import org.junit.Test

class WorkerTimingTest {
    @Test fun rareStallRemainsVisibleAndSnapshotsDoNotResetCounters() {
        val timing = WorkerTiming()
        assertEquals(0,timing.snapshot().count)
        repeat(98) { timing.record(50_000) }
        timing.record(10_000_000)
        timing.record(300_000_000)
        val summary = timing.snapshot()
        assertEquals(100,summary.count)
        assertEquals(0.125,summary.p50,0.0001)
        assertEquals(10.0,summary.p99,0.0001)
        assertEquals(300.0,summary.max,0.0001)
        assertEquals(0.05,timing.firstMs(),0.0001)
        assertEquals(summary,timing.snapshot())
    }
}
