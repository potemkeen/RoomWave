package com.roomwave.receiver

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.wifi.WifiManager
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import java.util.concurrent.atomic.AtomicBoolean

class AudioReceiverService : Service() {
    lateinit var advertiser: ReceiverAdvertiser
        private set
    var connection by mutableStateOf("Ожидание подключения ПК")
        private set
    var channelLabel by mutableStateOf<String?>(null)
        private set
    var preparing by mutableStateOf(false)
        private set
    var connected by mutableStateOf(false)
        private set
    var streamError by mutableStateOf<String?>(null)
        private set
    var packets by mutableStateOf(0L)
        private set
    var lost by mutableStateOf(0L)
        private set
    var latencyReport by mutableStateOf(LatencyReport(null, null, null))
        private set
    var syncReport by mutableStateOf(SyncReport())
        private set
    var stageMetrics by mutableStateOf(JSONObject())
        private set
    private val main = Handler(Looper.getMainLooper())
    private val running = AtomicBoolean(true)
    @Volatile private var server: ServerSocket? = null
    @Volatile private var client: Socket? = null
    private var worker: Thread? = null
    private val audioManager by lazy { getSystemService(AudioManager::class.java) }
    private val focus by lazy {
        AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setOnAudioFocusChangeListener({ change ->
                if (change < 0) { streamError = "Воспроизведение остановлено другим приложением или звонком"; disconnect() }
            }, main).build()
    }
    override fun onCreate() {
        super.onCreate()
        advertiser = ReceiverAdvertiser(applicationContext)
        lastError = null
        current = this
        getSystemService(NotificationManager::class.java).createNotificationChannel(NotificationChannel(CHANNEL, "Воспроизведение RoomWave", NotificationManager.IMPORTANCE_LOW))
        startForeground(1, notification("Ожидание подключения ПК"))
        worker = Thread({ listen() }, "RoomWave-Control").also { it.start() }
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == STOP) stopSelf()
        return START_NOT_STICKY
    }
    override fun onBind(intent: Intent?): IBinder? = null
    fun disconnect() { try { client?.close() } catch (e: Exception) { Log.e(TAG, "Disconnect failed", e) } }
    private fun status(text: String, isConnected: Boolean = false) { main.post { if (running.get()) { connection = text; connected = isConnected; getSystemService(NotificationManager::class.java).notify(1, notification(text)) } } }
    private fun notification(text: String): Notification {
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val stop = PendingIntent.getService(this, 1, Intent(this, AudioReceiverService::class.java).setAction(STOP), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL).setSmallIcon(R.drawable.ic_roomwave).setContentTitle("RoomWave")
            .setContentText(text).setContentIntent(open).setOngoing(true).setOnlyAlertOnce(true)
            .addAction(Notification.Action.Builder(null, "Остановить", stop).build()).build()
    }
    private fun listen() {
        try {
            val listener = ServerSocket(47801).also { server = it; it.soTimeout = 1000 }
            main.post { if (running.get()) advertiser.start() }
            while (running.get()) {
                val socket = try { listener.accept() } catch (_: SocketTimeoutException) { continue }
                client = socket
                try { session(socket) }
                catch (e: Exception) {
                    if (running.get() && !socket.isClosed) { Log.e(TAG, "Session ended: ${e.message}", e); main.post { streamError = e.message } }
                } finally {
                    try { socket.close() } catch (_: Exception) { }
                    client = null
                    main.post { preparing = false; channelLabel = null; latencyReport = LatencyReport(null, null, null); syncReport = SyncReport("disconnected") }
                    status("Ожидание подключения ПК")
                }
            }
        } catch (e: Exception) {
            if (running.get()) { Log.e(TAG, "Receiver server failed", e); main.post { lastError = e.message; stopSelf() } }
        } finally {
            try { server?.close() } catch (e: Exception) { Log.e(TAG, "Server close failed", e) }
        }
    }
    private fun readChannel(message: JSONObject): String? {
        val routing = message.optJSONObject("routing") ?: return null
        if (!routing.has("speaker")) return null
        val speaker = if (routing.isNull("speaker")) null else routing.optInt("speaker", -1)
        return channelDescription(speaker, routing.optBoolean("available", true))
    }
    private fun session(socket: Socket) {
        socket.soTimeout = 1000
        socket.tcpNoDelay = true
        val input = socket.getInputStream()
        val output = socket.getOutputStream()
        val line = ByteArrayOutputStream()
        var lastMessage = System.nanoTime()
        var lastStatsLog = 0L
        fun readMessage(): JSONObject? {
            while (running.get() && !socket.isClosed) {
                check(System.nanoTime() - lastMessage < 5_000_000_000L) { "Host heartbeat timed out" }
                val value = try { input.read() } catch (_: SocketTimeoutException) {
                    check(System.nanoTime() - lastMessage < 5_000_000_000L) { "Host heartbeat timed out" }
                    continue
                }
                if (value == -1) return null
                if (value == 10) {
                    val message = JSONObject(line.toString("UTF-8")); line.reset()
                    lastMessage = System.nanoTime()
                    return message
                }
                check(line.size() < 4096) { "Control message too large" }; line.write(value)
            }
            return null
        }
        fun reply(message: JSONObject) { output.write((message.toString() + "\n").toByteArray(Charsets.UTF_8)); output.flush() }
        val hello = readMessage() ?: return
        if (hello.optString("type") != "start" || hello.optInt("protocolVersion") != 4 || hello.optString("deviceId") != advertiser.deviceId || hello.optInt("sampleRate") != 48000 || hello.optInt("channels") != 2 || hello.optInt("framesPerPacket") != 240 || hello.optString("format") != "s16le") {
            reply(JSONObject().put("type", "error").put("message", "Unsupported RoomWave audio format")); return
        }
        val id = hello.getString("sessionId").toLong()
        check(id > 0) { "Invalid session" }
        main.post { preparing = true; streamError = null; syncReport = SyncReport() }
        status("Подключение к компьютеру…", isConnected = true)
        check(audioManager.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED) { "Audio focus denied" }
        val wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "RoomWave:Playback")
        @Suppress("DEPRECATION")
        val wifi = applicationContext.getSystemService(WifiManager::class.java).createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "RoomWave:WiFi")
        try {
            wake.setReferenceCounted(false)
            wake.acquire(60_000L)
            wifi.acquire()
            DeviceDiagnostics(this, { wake.isHeld }, { wifi.isHeld }).use { deviceDiagnostics ->
            PcmPlayer(id, socket.inetAddress, hello.optInt("targetDelayMs", 500)) { error ->
                Log.e(TAG, "Audio playback failed", error)
                main.post { streamError = error.message }
                disconnect()
            }.use { player ->
                player.start()
                val initialChannel = readChannel(hello)
                main.post { preparing = true; channelLabel = initialChannel; streamError = null; packets = 0; lost = 0; latencyReport = LatencyReport(null, null, null); syncReport = SyncReport() }
                status("Подключение к компьютеру…", isConnected = true)
                Log.i(TAG, "audio connected session=$id host=${socket.inetAddress.hostAddress}")
                reply(JSONObject().put("type", "ready").put("sessionId", id.toString()).put("extendedTiming", true).put("monoPcm", true).put("xorFec", true).put("pcm16k", true))
                var playbackReady = false
                while (running.get()) {
                    val message = readMessage() ?: break
                    when (message.optString("type")) {
                        "stop" -> break
                        "ping" -> {
                            val receiveNs = lastMessage
                            val updatedChannel = readChannel(message)
                            wake.acquire(60_000L)
                            player.syncClock(message.optString("clockOffsetNs").toLongOrNull(),
                                message.optDouble("rttMs").takeIf { it.isFinite() }, receiveNs)
                            val report = player.latency.report(System.nanoTime())
                            val synchronization = player.synchronization.get()
                            val readyNow = synchronization.status == "synced"
                            if (readyNow && !playbackReady) {
                                playbackReady = true
                                main.post { preparing = false }
                                status("Подключено: ${socket.inetAddress.hostAddress}", isConnected = true)
                            }
                            val stageReport = JSONObject(player.stageReport().toString()).put("deviceState",deviceDiagnostics.snapshot())
                            val receivedPackets = player.packets.get()
                            val missingPackets = player.lost.get()
                            val pong = JSONObject().put("type", "pong").put("packets", receivedPackets)
                                .put("lost", missingPackets).put("playedFrames", player.playedFrames.get())
                                .put("hostSendNs", message.getString("hostSendNs"))
                                .put("phoneReceiveNs", receiveNs.toString())
                                .put("latencyMs", report.latencyMs ?: JSONObject.NULL)
                                .put("rttMs", report.rttMs ?: JSONObject.NULL)
                                .put("latencyMethod", report.method ?: JSONObject.NULL)
                                .put("syncErrorMs", synchronization.errorMs ?: JSONObject.NULL)
                                .put("syncStatus", synchronization.status)
                                .put("stages",stageReport)
                                .put("phoneSendNs", System.nanoTime().toString())
                            reply(pong)
                            if (receiveNs - lastStatsLog >= 5_000_000_000L) {
                                Log.i(TAG, "stages=$stageReport sync=${synchronization.status} errorMs=${synchronization.errorMs} latencyMs=${report.latencyMs} rttMs=${report.rttMs} packets=$receivedPackets lost=$missingPackets")
                                lastStatsLog = receiveNs
                            }
                            main.post { if (updatedChannel != null) channelLabel = updatedChannel; packets = receivedPackets; lost = missingPackets; latencyReport = report; syncReport = synchronization; stageMetrics = stageReport }
                        }
                        else -> error("Unexpected control message")
                    }
                }
                Log.i(TAG, "audio disconnected session=$id packets=${player.packets.get()} playedFrames=${player.playedFrames.get()} lost=${player.lost.get()}")
            }
            }
        } finally {
            if (wifi.isHeld) wifi.release()
            if (wake.isHeld) wake.release()
            audioManager.abandonAudioFocusRequest(focus)
        }
    }
    override fun onDestroy() {
        running.set(false)
        advertiser.stop()
        disconnect()
        try { server?.close() } catch (e: Exception) { Log.e(TAG, "Server shutdown failed", e) }
        worker?.join(1500)
        current = null
        super.onDestroy()
    }
    companion object {
        var lastError by mutableStateOf<String?>(null)
            private set
        var current by mutableStateOf<AudioReceiverService?>(null)
            private set
        const val STOP = "com.roomwave.receiver.STOP"
        private const val CHANNEL = "roomwave_audio"
        private const val TAG = "RoomWaveAudio"
    }
}
