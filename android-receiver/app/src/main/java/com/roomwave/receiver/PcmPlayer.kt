package com.roomwave.receiver

import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.SocketTimeoutException
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject
import org.json.JSONArray
import kotlin.math.abs

/** Foreground-service-owned receiver. The native audio callback never enters the JVM. */
class PcmPlayer(private val sessionId: Long, private val host: InetAddress, private val targetDelayMs: Int = 500, private val onError: (Exception) -> Unit) : AutoCloseable {
    val packets = AtomicLong()
    val lost = AtomicLong()
    val playedFrames = AtomicLong()
    val latency = LatencyMeter()
    val synchronization = AtomicReference(SyncReport())
    private val clock = PlaybackClock()
    private val running = AtomicBoolean(true)
    private val socket = DatagramSocket(47800).apply { soTimeout = 2; receiveBufferSize = 128 * 1024 }
    private val handle = try { NativeAudio.open().also { check(it != 0L) { "AAudio could not open a 48 kHz stereo output" } } }
        catch (e: Exception) { socket.close(); throw e }
    private var reader: Thread? = null
    private var monitor: Thread? = null
    private val report = AtomicReference(JSONObject())
    private val decodeTiming = WorkerTiming()
    private val pushTiming = WorkerTiming()
    private val receiveTiming = WorkerTiming()
    @Volatile private var jitter = 0.0
    @Volatile private var extended = false
    @Volatile private var captureNs = 0L
    @Volatile private var offsetNs = 0L
    @Volatile private var rtt = 10.0
    @Volatile private var outputLead = 40.0
    private val retransmitRequests = AtomicLong()
    private val gapsDetected = AtomicLong()
    private val repairRateLimited = AtomicLong()
    private val repairStallGraceNs = AtomicLong(18_000_000)
    private val gapsPredicted = AtomicLong()
    private val repairDeadlineExpired = AtomicLong()
    private val repairExpiredWithoutRequest = AtomicLong()
    private val recovered = AtomicLong()
    private val originalsReceived = AtomicLong()
    private val qualityReceived = AtomicLong()
    private val latencyReceived = AtomicLong()
    private val wireBytes = AtomicLong()
    @Volatile private var wireRate=48000
    private val repairsReceived = AtomicLong()
    private val fecAccepted = AtomicLong()
    private val rejected = AtomicLong()
    private val fecRecovered = AtomicLong()
    private val fecInvalid = AtomicLong()
    @Volatile private var transitSummary = DelaySummary()
    @Volatile private var repairSummary = DelaySummary()
    fun stageReport(): JSONObject = report.get()
    fun syncClock(offset: Long?, rttMs: Double?, now: Long) {
        val effective = clock.update(offset, rttMs, now)
        latency.sync(effective, rttMs, now)
        if(rttMs != null && rttMs.isFinite()) rtt = rttMs
        if (effective != null) { offsetNs = effective; NativeAudio.clock(handle,effective,now) }
    }
    private fun guarded(action: () -> Unit) {
        try { action() } catch (e: Exception) { if (running.getAndSet(false)) onError(e) }
    }
    fun start() {
        reader = Thread({ guarded { receive() } }, "RoomWave-UDP").also { it.start() }
        monitor = Thread({ guarded { measure() } }, "RoomWave-AAudio-Metrics").also { it.start() }
    }
    private fun receive() {
        val bytes = ByteArray(1500)
        // NativeAudio.push copies synchronously; FEC retains wire bytes, never this PCM.
        val pcmScratch = ByteArray(960)
        val datagram = DatagramPacket(bytes,bytes.size)
        var previousTransit: Double? = null
        val tracker = RepairTracker()
        val missing = tracker.missing
        val fec=PcmFec(sessionId)
        val transitWindow=DelayWindow()
        val repairWindow=DelayWindow(128)
        var lastSummary=0L
        var peerPort = 0
        val nackBytes = ByteArray(24)
        val nack = ByteBuffer.wrap(nackBytes).order(ByteOrder.BIG_ENDIAN).apply {
            putInt(0x52574e41); putInt(4); putLong(sessionId)
        }
        fun acceptWire(wire: ByteArray, length: Int, received: Long, fecRepair: Boolean): AudioPacket? {
            val decodeStart = System.nanoTime()
            val packet=decode(wire,length,sessionId,pcmScratch)
            decodeTiming.record(System.nanoTime()-decodeStart)
            if(packet == null) return null
            wireRate=packet.sampleRate
            if(!fecRepair) {wireBytes.addAndGet(length.toLong());if(packet.sampleRate==48000) qualityReceived.incrementAndGet() else latencyReceived.incrementAndGet()}
            val retransmitted=(wire[7].toInt() and 2)!=0
            val kind=if(fecRepair) 2 else if(retransmitted) 1 else 0
            val gap=tracker.observe(packet.frame,packet.playNs,received,kind==0)
            if(kind==0) originalsReceived.incrementAndGet() else repairsReceived.incrementAndGet()
            if(retransmitted && gap!=null && gap.attempts>0) repairWindow.add((received-gap.lastRequest)/1e6)
            val send=packet.sendNs.takeIf { it>0 } ?: (packet.playNs-targetDelayMs*1_000_000L)
            if(kind==0) clock.localTime(send,received)?.let {
                val transit=(received-it)/1e6
                transitWindow.add(transit)
                previousTransit?.let { old -> jitter+=(abs(transit-old)-jitter)/16 }
                previousTransit=transit
            }
            extended=packet.readNs>0
            val capture=if(extended) packet.readNs else packet.captureNs
            captureNs=capture
            val pushStart = System.nanoTime()
            val accepted = NativeAudio.push(handle,packet.frame,capture,send,packet.playNs,received,packet.pcm,kind,packet.sampleRate)
            pushTiming.record(System.nanoTime()-pushStart)
            if(accepted) {
                if(kind!=0) recovered.incrementAndGet()
                if(kind==2) fecAccepted.incrementAndGet()
            } else rejected.incrementAndGet()
            return packet
        }
        while (running.get()) {
            var receiveStart = 0L
            try {
                datagram.length=bytes.size; socket.receive(datagram)
                val received=System.nanoTime()
                if(datagram.address!=host) continue
                receiveStart = received
                val parity=datagram.length>=4 && bytes[0]==82.toByte() && bytes[1]==87.toByte() && bytes[2]==70.toByte() && bytes[3]==88.toByte()
                val repaired=if(parity) fec.parity(bytes,datagram.length) else {
                    val packet=acceptWire(bytes,datagram.length,received,false)
                    if(packet!=null) {
                        peerPort=datagram.port
                        // FEC protects originals. Retransmissions retain those bytes except the marker.
                        val flags=bytes[7]; bytes[7]=(flags.toInt() and 2.inv()).toByte()
                        val result=fec.original(bytes,datagram.length,packet.frame)
                        bytes[7]=flags; result
                    } else null
                }
                if(repaired!=null) acceptWire(repaired,repaired.size,received,true)
                fecRecovered.set(fec.recovered); fecInvalid.set(fec.invalid)
            } catch (_: SocketTimeoutException) {
                // A render endpoint can disappear temporarily. TCP heartbeats own session
                // liveness; the native renderer fades silence and rejoins host deadlines.
            }
            val now = System.nanoTime()
            if(now-lastSummary>=500_000_000) {
                transitSummary=transitWindow.snapshot(); repairSummary=repairWindow.snapshot(); lastSummary=now
            }
            if (extended && peerPort > 0) tracker.predict(now)
            gapsDetected.set(tracker.detected); gapsPredicted.set(tracker.predicted)
            repairRateLimited.set(tracker.rateLimited); repairStallGraceNs.set(tracker.stallGraceNs)
            val iterator = missing.entries.iterator()
            var requests = 0
            while(iterator.hasNext()) {
                val (frame,gap) = iterator.next()
                val deadline = clock.localTime(gap.deadline,now) ?: continue
                val repairRtt=if(repairSummary.count>=4) maxOf(rtt,repairSummary.p95) else rtt
                if(!RepairTracker.canRepair(deadline,now,outputLead,repairRtt)) {
                    repairDeadlineExpired.incrementAndGet()
                    if(gap.attempts==0) repairExpiredWithoutRequest.incrementAndGet()
                    iterator.remove(); continue
                }
                if(extended && peerPort > 0 && now-gap.detected>=2_000_000 && gap.attempts < 2 && requests < 4 && now-gap.lastRequest >= (maxOf(repairRtt,5.0)*1e6).toLong()) {
                    if (!tracker.allowRequest(now)) break
                    nack.putLong(16,frame)
                    socket.send(DatagramPacket(nackBytes,nackBytes.size,host,peerPort))
                    gap.attempts++; gap.lastRequest=now; requests++; retransmitRequests.incrementAndGet()
                }
            }
            if(receiveStart != 0L) receiveTiming.record(System.nanoTime()-receiveStart)
        }
    }
    private fun measure() {
        while(running.get()) {
            val s = NativeAudio.poll(handle)
            check(s[16] == 0.0) { "AAudio output disconnected: ${s[16].toInt()}" }
            packets.set(s[0].toLong()); lost.set(s[1].toLong()); playedFrames.set(s[3].toLong())
            val now = System.nanoTime()
            outputLead = s[6].coerceIn(10.0,500.0)
            val state = when(s[15].toInt()) { 0 -> "audioClock"; 1 -> "buffering"; 2 -> "aligning"; else -> "synced" }
            synchronization.set(SyncReport(state,s[4].takeIf { s[15] >= 2 }))
            if(s[7] in 0.0..5000.0 && s[15] >= 2 && captureNs > 0) {
                val capture = captureNs
                latency.record(capture,capture+offsetNs+(s[7]*1e6).toLong(),if(extended) "capture-read" else "timestamp",now)
            }
            fun timing(value: WorkerTiming): JSONObject {
                val summary = value.snapshot()
                return JSONObject().put("samples",summary.count).put("p50Ms",summary.p50)
                    .put("p95Ms",summary.p95).put("p99Ms",summary.p99).put("maxMs",summary.max).put("firstMs",value.firstMs())
            }
            report.set(JSONObject().put("receiverPolicy","burst-repair-v3")
                .put("decodeProcessing",timing(decodeTiming)).put("nativePushProcessing",timing(pushTiming))
                .put("receiveProcessing",timing(receiveTiming)).put("captureToSendMs",s[9]).put("networkMs",s[8])
                .put("jitterBufferMs",s[5]).put("audioOutputMs",s[6]).put("jitterMs",jitter)
                .put("latePackets",s[2].toLong()).put("packetLoss",s[1].toLong()).put("underruns",s[10].toLong())
                .put("repairRateLimited",repairRateLimited.get()).put("repairStallGraceMs",repairStallGraceNs.get()/1e6)
                .put("repairGapsDetected",gapsDetected.get()).put("repairGapsPredicted",gapsPredicted.get())
                .put("repairDeadlineExpired",repairDeadlineExpired.get()).put("repairExpiredWithoutRequest",repairExpiredWithoutRequest.get())
                .put("nackRequests",retransmitRequests.get()).put("recoveredPackets",recovered.get())
                .put("originalsReceived",originalsReceived.get()).put("repairsReceived",repairsReceived.get())
                .put("nativeRejected",rejected.get()).put("fecRecovered",fecRecovered.get()).put("fecAccepted",fecAccepted.get())
                .put("fecInvalid",fecInvalid.get()).put("playedPackets",s[17].toLong())
                .put("fecPlayed",s[18].toLong()).put("retransmitPlayed",s[19].toLong())
                .put("duplicates",s[20].toLong()).put("queueCollisions",s[21].toLong())
                .put("transitP50Ms",transitSummary.p50).put("transitP95Ms",transitSummary.p95)
                .put("transitP99Ms",transitSummary.p99).put("transitMaxMs",transitSummary.max)
                .put("repairRttP95Ms",repairSummary.p95).put("repairRttSamples",repairSummary.count)
                .put("suggestedJitterMs",transitSummary.jitterBudgetMs())
                .put("hardwareBufferFrames",s[11].toInt()).put("framesPerBurst",s[12].toInt())
                .put("performanceMode",s[13].toInt()).put("sharingMode",s[14].toInt())
                .put("streamMode",if(wireRate==16000) "latency" else "quality").put("wireSampleRate",wireRate).put("resamplerDelayMs",if(wireRate==16000) 62.0/48 else 0.0)
                .put("qualityPacketsReceived",qualityReceived.get()).put("latencyPacketsReceived",latencyReceived.get()).put("audioWireBytesReceived",wireBytes.get())
                .put("playbackSampleRate",s[30].toInt()).put("qualityPlayed",s[31].toLong()).put("latencyPlayed",s[32].toLong())
                .put("qualityLost",s[33].toLong()).put("latencyLost",s[34].toLong())
                .put("audioPolicy","verified-low-latency-v2")
                .put("openAttempts",s[35].toInt()).put("selectedOpenPolicy",s[36].toInt())
                .put("lowLatencyGranted",s[13].toInt()==12)
                .put("openCandidates",JSONArray().also { a ->
                    for(i in 0..3) { val j=37+i*5
                        if(s[j]!=-1.0) a.put(JSONObject().put("usage",if(i<2) "game" else "media")
                            .put("requestedSharing",if(i%2==0) "exclusive" else "shared")
                            .put("result",s[j].toInt()).put("performanceMode",s[j+1].toInt())
                            .put("sharingMode",s[j+2].toInt()).put("framesPerBurst",s[j+3].toInt())
                            .put("capacityFrames",s[j+4].toInt()))
                    }
                }).put("exclusiveOpenResult",s[22].toInt())
                .put("gameUsageRequested",s[23]!=0.0).put("sharedOpenRetried",s[24]!=0.0)
                .put("mediaUsageFallback",s[25]!=0.0).put("actualUsage",s[26].toInt())
                .put("actualSampleRate",s[27].toInt()).put("audioDeviceId",s[28].toInt()).put("bufferCapacityFrames",s[29].toInt())
                .put("sendTimeEstimated",!extended).put("captureMethod",if(extended) "wasapi-read" else "driver-timestamp")
                .put("latencyScope",if(extended) "host-pcm-read-to-presentation" else "driver-timestamp-to-presentation")
                .put("preCaptureLatencyMeasured",false)
                .put("outputBackend","AAudio").put("recommendedJitterMs",(20+4*jitter).coerceIn(20.0,50.0)))
            Thread.sleep(50)
        }
    }
    override fun close() {
        running.set(false); socket.close()
        reader?.join(); monitor?.join()
        NativeAudio.close(handle)
    }
    companion object {
        fun decode(bytes: ByteArray, length: Int, sessionId: Long, pcmOutput: ByteArray = ByteArray(960)): AudioPacket? {
            if(pcmOutput.size != 960) return null
            if ((length != 1008 && length != 1024 && length != 544 && length != 266 && length != 468) || bytes.size < length) return null
            val b = ByteBuffer.wrap(bytes,0,length).order(ByteOrder.BIG_ENDIAN)
            if (b.int != 0x52574156 || b.get().toInt() != 4 || b.get().toInt() != 1) return null
            val channels=b.get().toInt()
            if(channels !in 1..2) return null
            val flags = b.get().toInt()
            val low=flags==5 || flags==7
            val extended=flags==1 || flags==3 || low
            if(flags==0) {if(channels!=2 || length!=1008) return null}
            else if(!extended || length!=64+(if(low) 101 else 240)*channels*2) return null
            if (b.long != sessionId) return null
            val sequence = b.int.toLong() and 0xffffffffL
            val frame = b.long
            if (frame < 0 || frame % 240 != 0L || (frame / 240 and 0xffffffffL) != sequence || b.short.toInt() != 240 || b.short.toInt() != (if(low) 16000 else 0)) return null
            val capture = b.long; val play = b.long
            if (capture < 0 || play <= 0 || (flags == 0 && capture > 0 && play < capture)) return null
            val send = if(extended) b.long else 0
            val read = if(extended) b.long else 0
            if(extended && (send <= 0 || read <= 0 || read > send || send >= play)) return null
            val pcm=if(low) Pcm16k.expand(bytes,channels,pcmOutput) else pcmOutput.also { out ->
                if(channels==2) bytes.copyInto(out,0,if(extended) 64 else 48,length)
                else for(n in 0 until 240) { out[n*4]=bytes[64+n*2]; out[n*4+1]=bytes[65+n*2]; out[n*4+2]=out[n*4]; out[n*4+3]=out[n*4+1] }
            }
            return AudioPacket(frame,capture,play,pcm,send,read,if(low) 16000 else 48000)
        }
    }
}
data class AudioPacket(val frame: Long,val captureNs: Long,val playNs: Long,val pcm: ByteArray,val sendNs: Long=0,val readNs: Long=0,val sampleRate: Int=48000)
