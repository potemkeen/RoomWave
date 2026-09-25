package com.roomwave.receiver

import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicLongArray
import kotlin.math.ceil

/** One writer (UDP), one reader (metrics). Fixed storage; no sorting/allocation on record. */
class WorkerTiming {
    private val buckets = AtomicLongArray(257)
    private val count = AtomicLong()
    private val maximum = AtomicLong()
    private val first = AtomicLong(-1)

    fun record(ns: Long) {
        val value = ns.coerceAtLeast(0)
        first.compareAndSet(-1, value)
        // 0.125 ms buckets; the final bucket represents values above 32 ms.
        val index = if(value > 32_000_000L) 256 else ((value + 124_999) / 125_000).toInt()
        buckets.incrementAndGet(index)
        if(value > maximum.get()) maximum.set(value) // Single writer.
        count.incrementAndGet()
    }

    fun snapshot(): DelaySummary {
        val n = count.get()
        if(n == 0L) return DelaySummary()
        val maxMs = maximum.get() / 1e6
        fun percentile(q: Double): Double {
            val rank = ceil(n*q).toLong()
            var total = 0L
            for(i in 0 until buckets.length()) {
                total += buckets.get(i)
                if(total >= rank) return if(i == 256) maxMs else minOf(i * 0.125, maxMs)
            }
            return maxMs
        }
        return DelaySummary(n.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), percentile(0.5), percentile(0.95), percentile(0.99), maxMs)
    }

    fun firstMs(): Double = first.get().coerceAtLeast(0) / 1e6
}
