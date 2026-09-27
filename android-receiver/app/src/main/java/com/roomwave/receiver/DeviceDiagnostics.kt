package com.roomwave.receiver

import android.app.ActivityManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.PowerManager
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicReference

/** System queries never run on the UDP/audio/control threads. Snapshots are immutable strings. */
class DeviceDiagnostics(context: Context, private val wakeHeld: () -> Boolean, private val wifiHeld: () -> Boolean) : AutoCloseable {
    private val app = context.applicationContext
    private val power = app.getSystemService(PowerManager::class.java)
    private val network = app.getSystemService(ConnectivityManager::class.java)
    private val wifi = app.getSystemService(WifiManager::class.java)
    private val thread = HandlerThread("RoomWave-DeviceMetrics").apply { start() }
    private val handler = Handler(thread.looper)
    private val latest = AtomicReference("{}")
    private val events = ArrayDeque<JSONObject>()
    private var revision = 0L
    private var previousNetwork: String? = null
    @Volatile private var closed = false
    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if(closed) return
            event(intent?.action?.substringAfterLast('.') ?: "unknown")
            sample()
        }
    }
    private val poll = object : Runnable {
        override fun run() {
            if(closed) return
            sample()
            handler.postDelayed(this,1000)
        }
    }

    init {
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_ON); addAction(Intent.ACTION_SCREEN_OFF)
            addAction(Intent.ACTION_POWER_CONNECTED); addAction(Intent.ACTION_POWER_DISCONNECTED)
            addAction(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED)
            addAction(PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED)
        }
        if(Build.VERSION.SDK_INT >= 33) app.registerReceiver(receiver,filter,null,handler,Context.RECEIVER_NOT_EXPORTED)
        else app.registerReceiver(receiver,filter,null,handler)
        handler.post { event("initial"); poll.run() }
    }

    private fun event(kind: String) {
        revision++
        if(events.size == 8) events.removeFirst()
        events.addLast(JSONObject().put("revision",revision).put("kind",kind)
            .put("unixMs",System.currentTimeMillis()).put("phoneMonoNs",System.nanoTime().toString())
            .put("screenInteractive",power.isInteractive))
    }

    private fun sample() {
        try { sampleInternal() } catch(e: Exception) {
            latest.set(JSONObject().put("sampleUnixMs",System.currentTimeMillis())
                .put("queryError",e.javaClass.simpleName).toString())
        }
    }

    @Suppress("DEPRECATION")
    private fun sampleInternal() {
        val value = JSONObject().put("schema",1).put("sampleUnixMs",System.currentTimeMillis())
            .put("phoneMonoNs",System.nanoTime().toString()).put("androidApi",Build.VERSION.SDK_INT)
            .put("androidRelease",Build.VERSION.RELEASE).put("manufacturer",Build.MANUFACTURER)
            .put("screenInteractive",power.isInteractive).put("powerSave",power.isPowerSaveMode)
            .put("deviceIdle",power.isDeviceIdleMode)
            .put("ignoringBatteryOptimizations",power.isIgnoringBatteryOptimizations(app.packageName))
            .put("partialWakeLockHeld",wakeHeld()).put("wifiLockHeld",wifiHeld())
            .put("wifiLockRequested","HIGH_PERF").put("eventRevision",revision)
            .put("wifiLockScreenRestricted",Build.VERSION.SDK_INT>=34)
            .put("wifiLockEffectiveMode",if(Build.VERSION.SDK_INT>=34) "LOW_LATENCY" else "HIGH_PERF")
            .put("keepScreenDuringPlayback",app.getSharedPreferences("playback",Context.MODE_PRIVATE).getBoolean("keepScreenDuringPlayback",false))
        try {
            val battery = app.registerReceiver(null,IntentFilter(Intent.ACTION_BATTERY_CHANGED))
            value.put("plugged",battery?.getIntExtra(BatteryManager.EXTRA_PLUGGED,-1) ?: JSONObject.NULL)
            value.put("batteryLevel",battery?.getIntExtra(BatteryManager.EXTRA_LEVEL,-1) ?: JSONObject.NULL)
            val process = ActivityManager.RunningAppProcessInfo()
            ActivityManager.getMyMemoryState(process)
            value.put("processImportance",process.importance)
            // Necessary conditions, not proof that the vendor driver applied a lock.
            value.put("wifiLowLatencyEligible",power.isInteractive && process.importance==ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND)
            if(Build.VERSION.SDK_INT >= 28) value.put("backgroundRestricted",app.getSystemService(ActivityManager::class.java).isBackgroundRestricted)
            val active = network.activeNetwork
            val capabilities = active?.let { network.getNetworkCapabilities(it) }
            val key = active?.networkHandle?.toString() ?: "none"
            if(previousNetwork != key) { previousNetwork = key; event("defaultNetworkChanged") }
            value.put("networkHandle",key).put("wifiTransport",capabilities?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ?: false)
                .put("vpnTransport",capabilities?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) ?: false)
            // No SSID/BSSID/location permission requested. These may be redacted by Android.
            val info = wifi.connectionInfo
            value.put("wifiFrequencyMhz",info?.frequency?.takeIf { it > 0 } ?: JSONObject.NULL)
                .put("wifiRssiDbm",info?.rssi?.takeIf { it > -127 && it < 0 } ?: JSONObject.NULL)
                .put("wifiLinkMbps",info?.linkSpeed?.takeIf { it > 0 } ?: JSONObject.NULL)
        } catch(e: Exception) {
            value.put("queryError",e.javaClass.simpleName)
        }
        value.put("eventRevision",revision).put("recentEvents",JSONArray(events.toList()))
        latest.set(value.toString())
    }

    fun snapshot(): JSONObject = JSONObject(latest.get())

    override fun close() {
        closed = true
        app.unregisterReceiver(receiver)
        handler.removeCallbacksAndMessages(null)
        thread.quitSafely()
        thread.join(1500)
    }
}
