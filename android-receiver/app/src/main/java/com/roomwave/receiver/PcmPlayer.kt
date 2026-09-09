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
import kotlin.math.abs
import java.util.TreeMap

/** Foreground-service-owned receiver. The native audio callback never enters the JVM. */
class PcmPlayer(private val sessionId: Long, private val host: InetAddress, private val targetDelayMs: Int = 500, private val onError: (Exception) -> Unit) : AutoCloseable {
    val packets = AtomicLong()
    val lost = AtomicLong()
    val playedFrames = AtomicLong()
    val latency = LatencyMeter()
    val synchronization = AtomicReference(SyncReport())
    private val clock = PlaybackClock()
    private val running = AtomicBoolean(true)
    private val socket = DatagramSocket(47800).apply { soTimeout = 5; receiveBufferSize = 128 * 1024 }
    private val handle = try { NativeAudio.open().also { check(it != 0L) { "AAudio could not open a 48 kHz stereo output" } } }
        catch (e: Exception) { socket.close(); throw e }
    private var reader: Thread? = null
    private var monitor: Thread? = null
    private val report = AtomicReference(JSONObject())
    @Volatile private var jitter = 0.0
    @Volatile private var extended = false
    @Volatile private var captureNs = 0L
    @Volatile private var offsetNs = 0L
    @Volatile private var rtt = 10.0
    @Volatile private var outputLead = 40.0
    private val retransmitRequests = AtomicLong()
    private val recovered = AtomicLong()
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
        val datagram = DatagramPacket(bytes,bytes.size)
        var previousTransit: Double? = null
        data class Gap(val deadline: Long, var lastRequest: Long = 0, var attempts: Int = 0)
        val missing = TreeMap<Long,Gap>()
        var highest = -240L
        var peerPort = 0
        val nackBytes = ByteArray(24)
        val nack = ByteBuffer.wrap(nackBytes).order(ByteOrder.BIG_ENDIAN).apply {
            putInt(0x52574e41); putInt(4); putLong(sessionId)
        }
        while (running.get()) {
            try {
                datagram.length = bytes.size; socket.receive(datagram)
                val received = System.nanoTime()
                if (datagram.address != host) continue
                val packet = decode(bytes,datagram.length,sessionId) ?: continue
                peerPort = datagram.port
                if(missing.remove(packet.frame)?.attempts?.let { it > 0 } == true) recovered.incrementAndGet()
                if(highest >= 0 && packet.frame > highest + 240 && packet.frame - highest <= 240 * 32) {
                    var frame = highest + 240
                    while(frame < packet.frame && missing.size < 32) {
                        missing.putIfAbsent(frame,Gap(packet.playNs-(packet.frame-frame)*1_000_000_000L/48000))
                        frame += 240
                    }
                }
                highest = maxOf(highest,packet.frame)
                val send = packet.sendNs.takeIf { it > 0 } ?: (packet.playNs - targetDelayMs * 1_000_000L)
                clock.localTime(send,received)?.let {
                    val transit = (received-it)/1e6
                    previousTransit?.let { old -> jitter += (abs(transit-old)-jitter)/16 }
                    previousTransit = transit
                }
                extended = packet.readNs > 0
                val capture = if (extended) packet.readNs else packet.captureNs
                captureNs = capture
                NativeAudio.push(handle,packet.frame,capture,send,packet.playNs,received,packet.pcm)
            } catch (_: SocketTimeoutException) {
                // A render endpoint can disappear temporarily. TCP heartbeats own session
                // liveness; the native renderer fades silence and rejoins host deadlines.
            }
            val now = System.nanoTime()
            val iterator = missing.entries.iterator()
            var requests = 0
            while(iterator.hasNext()) {
                val (frame,gap) = iterator.next()
                val deadline = clock.localTime(gap.deadline,now) ?: continue
                if(deadline-now <= ((outputLead+rtt+4)*1e6).toLong()) { iterator.remove(); continue }
                if(extended && peerPort > 0 && gap.attempts < 2 && requests < 4 && now-gap.lastRequest >= (maxOf(rtt,5.0)*1e6).toLong()) {
                    nack.putLong(16,frame)
                    socket.send(DatagramPacket(nackBytes,nackBytes.size,host,peerPort))
                    gap.attempts++; gap.lastRequest=now; requests++; retransmitRequests.incrementAndGet()
                }
            }
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
            report.set(JSONObject().put("captureToSendMs",s[9]).put("networkMs",s[8])
                .put("jitterBufferMs",s[5]).put("audioOutputMs",s[6]).put("jitterMs",jitter)
                .put("latePackets",s[2].toLong()).put("packetLoss",s[1].toLong()).put("underruns",s[10].toLong())
                .put("nackRequests",retransmitRequests.get()).put("recoveredPackets",recovered.get())
                .put("hardwareBufferFrames",s[11].toInt()).put("framesPerBurst",s[12].toInt())
                .put("performanceMode",s[13].toInt()).put("sharingMode",s[14].toInt())
                .put("sendTimeEstimated",!extended).put("captureMethod",if(extended) "wasapi-read" else "driver-timestamp")
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
        fun decode(bytes: ByteArray, length: Int, sessionId: Long): AudioPacket? {
            if ((length != 1008 && length != 1024 && length != 544) || bytes.size < length) return null
            val b = ByteBuffer.wrap(bytes,0,length).order(ByteOrder.BIG_ENDIAN)
            if (b.int != 0x52574156 || b.get().toInt() != 4 || b.get().toInt() != 1) return null
            val channels=b.get().toInt()
            if(channels !in 1..2) return null
            val flags = b.get().toInt()
            if ((flags != 0 || channels != 2 || length != 1008) && (flags != 1 || length != 64+240*channels*2)) return null
            if (b.long != sessionId) return null
            val sequence = b.int.toLong() and 0xffffffffL
            val frame = b.long
            if (frame < 0 || frame % 240 != 0L || (frame / 240 and 0xffffffffL) != sequence || b.short.toInt() != 240 || b.short.toInt() != 0) return null
            val capture = b.long; val play = b.long
            if (capture < 0 || play <= 0 || (flags == 0 && capture > 0 && play < capture)) return null
            val send = if(flags == 1) b.long else 0
            val read = if(flags == 1) b.long else 0
            if(flags == 1 && (send <= 0 || read <= 0 || read > send || send >= play)) return null
            val pcm=if(channels==2) bytes.copyOfRange(if(flags==1) 64 else 48,length) else ByteArray(960).also { out ->
                for(n in 0 until 240) { out[n*4]=bytes[64+n*2]; out[n*4+1]=bytes[65+n*2]; out[n*4+2]=out[n*4]; out[n*4+3]=out[n*4+1] }
            }
            return AudioPacket(frame,capture,play,pcm,send,read)
        }
    }
}
data class AudioPacket(val frame: Long,val captureNs: Long,val playNs: Long,val pcm: ByteArray,val sendNs: Long=0,val readNs: Long=0)
