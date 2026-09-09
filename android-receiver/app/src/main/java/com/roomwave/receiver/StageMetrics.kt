package com.roomwave.receiver

import org.json.JSONObject
import kotlin.math.abs

/** Diagnostic work stays off the real-time path in the native implementation. Units: milliseconds. */
class StageMetrics {
    private var captureToSend = Double.NaN
    private var network = Double.NaN
    private var jitterBuffer = Double.NaN
    private var output = Double.NaN
    private var lastTransit = Double.NaN
    private var jitter = 0.0
    private var samples = 0L
    private var late = 0L
    private var missing = 0L
    private var raw = doubleArrayOf(Double.NaN, Double.NaN, Double.NaN, Double.NaN)
    private fun smooth(old: Double, value: Double) = if (old.isNaN()) value else old * 0.9 + value * 0.1
    @Synchronized fun arrival(transitMs: Double) {
        if (!transitMs.isFinite() || transitMs !in -20.0..2000.0) return
        if (lastTransit.isFinite()) jitter += (abs(transitMs - lastTransit) - jitter) / 16
        lastTransit = transitMs
    }
    @Synchronized fun late() { late++ }
    @Synchronized fun missing() { missing++ }
    @Synchronized fun sample(captureNs: Long, sendNs: Long, receiveNs: Long, localSendNs: Long, submitNs: Long, presentationNs: Long) {
        if (captureNs <= 0) return
        val a = (sendNs - captureNs) / 1e6
        val b = (receiveNs - localSendNs) / 1e6
        val c = (submitNs - receiveNs) / 1e6
        val d = (presentationNs - submitNs) / 1e6
        raw[0] = a; raw[1] = b; raw[2] = c; raw[3] = d
        if (a.isFinite() && a in -20.0..2000.0) captureToSend = smooth(captureToSend,a)
        if (b.isFinite() && b in -20.0..2000.0) network = smooth(network,b)
        if (c.isFinite() && c in -20.0..2000.0) jitterBuffer = smooth(jitterBuffer,c)
        if (d.isFinite() && d in -20.0..2000.0) output = smooth(output,d)
        samples++
    }
    @Synchronized fun json(underruns: Int, hardwareFrames: Int): JSONObject = JSONObject()
        .put("captureToSendMs", captureToSend.takeIf { it.isFinite() } ?: JSONObject.NULL)
        .put("networkMs", network.takeIf { it.isFinite() } ?: JSONObject.NULL)
        .put("jitterBufferMs", jitterBuffer.takeIf { it.isFinite() } ?: JSONObject.NULL)
        .put("audioOutputMs", output.takeIf { it.isFinite() } ?: JSONObject.NULL)
        .put("jitterMs", jitter).put("latePackets",late).put("packetLoss",missing)
        .put("underruns",underruns).put("hardwareBufferFrames",hardwareFrames).put("stageSamples",samples)
        .put("sendTimeEstimated",true)
        .put("rawStagesMs", org.json.JSONArray(raw.map { it.takeIf { value -> value.isFinite() } ?: JSONObject.NULL }))
}
