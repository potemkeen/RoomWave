package com.roomwave.receiver

import java.util.TreeMap

/** Bounded network-thread tracker, using the sender timeline even during a delivery stall. */
class RepairTracker {
    data class Gap(val deadline: Long, val detected: Long, var lastRequest: Long = 0, var attempts: Int = 0)
    val missing = TreeMap<Long, Gap>()
    var detected = 0L; private set
    var predicted = 0L; private set
    private var highest = -1L
    private var play = 0L
    private var arrival = 0L
    private var nextPrediction = 0L
    private val intervals = LongArray(64)
    private var intervalCount = 0
    private var intervalIndex = 0
    private var tokens = 4.0
    private var tokenAt = -1L
    var rateLimited = 0L; private set
    var stallGraceNs = 18_000_000L; private set
    fun allowRequest(now: Long): Boolean {
        if (tokenAt >= 0) tokens = minOf(4.0, tokens + (now-tokenAt).coerceAtLeast(0)/1e9*10)
        tokenAt=now
        if (tokens < 1) { rateLimited++; return false }
        tokens-=1
        return true
    }
    private fun add(frame: Long, deadline: Long, now: Long, prediction: Boolean) {
        if (missing.size < 32 && missing.putIfAbsent(frame, Gap(deadline, now)) == null) {
            detected++
            if (prediction) predicted++
        }
    }
    fun observe(frame: Long, deadline: Long, now: Long, original: Boolean = true): Gap? {
        val gap = missing.remove(frame)
        if (!original) return gap
        if (frame > highest) {
            val interval=now-arrival
            if (highest >= 0 && interval in 2_000_000L..100_000_000L) {
                intervals[intervalIndex]=interval; intervalIndex=(intervalIndex+1)%intervals.size
                intervalCount=minOf(intervalCount+1,intervals.size)
                val sorted=intervals.copyOf(intervalCount).sorted()
                stallGraceNs=(sorted[(intervalCount-1)*95/100]+2_000_000).coerceIn(18_000_000,50_000_000)
            }
            if (highest >= 0 && frame-highest <= 240*32) {
                var f = highest+240
                while (f < frame) { add(f, deadline-(frame-f)/240*5_000_000, now, false); f+=240 }
            }
            highest=frame; play=deadline; arrival=now; nextPrediction=frame+240
        }
        return gap
    }
    fun predict(now: Long) {
        if (highest < 0) return
        // Audio is 5 ms per packet, but WASAPI/network delivery comes in bursts.
        // Wait for a missing burst, not the nominal duration of one packet.
        while (nextPrediction <= highest+32*240 &&
            now-arrival >= stallGraceNs+(nextPrediction-highest-240)/240*5_000_000) {
            add(nextPrediction, play+(nextPrediction-highest)/240*5_000_000, now-2_000_000, true)
            nextPrediction+=240
        }
    }
    companion object {
        fun canRepair(deadline: Long, now: Long, outputMs: Double, rttMs: Double): Boolean =
            deadline-now > ((outputMs+rttMs+4)*1e6).toLong()
    }
}
