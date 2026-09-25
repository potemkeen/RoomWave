package com.roomwave.receiver

import kotlin.math.ceil

/** Bounded network-worker statistics. Snapshot sorting never runs in the audio callback. */
class DelayWindow(private val capacity: Int = 2048) {
    private val values = DoubleArray(capacity)
    private var size = 0
    private var next = 0
    fun add(ms: Double) {
        if (!ms.isFinite()) return
        values[next] = ms; next = (next+1)%capacity; size = minOf(size+1,capacity)
    }
    fun snapshot(): DelaySummary {
        if(size==0) return DelaySummary()
        val sorted=values.copyOf(size).apply { sort() }
        fun p(q: Double) = sorted[(ceil(size*q).toInt()-1).coerceIn(0,size-1)]
        return DelaySummary(size,p(0.5),p(0.95),p(0.99),sorted.last(),(p(0.999)-p(0.01)).coerceAtLeast(0.0))
    }
}
data class DelaySummary(val count: Int=0,val p50: Double=0.0,val p95: Double=0.0,val p99: Double=0.0,val max: Double=0.0,val tail: Double=0.0) {
    // Shadow recommendation until receiver/output margins are validated on hardware.
    fun jitterBudgetMs() = if(count<200) 20.0 else (tail+12.0).coerceIn(12.0,50.0)
}
