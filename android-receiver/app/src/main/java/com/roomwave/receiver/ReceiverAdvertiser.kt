package com.roomwave.receiver

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.net.Inet4Address
import java.util.UUID

/** All lifecycle and NSD callback state is serialized on the main thread. */
class ReceiverAdvertiser(context: Context) {
    private val main = Handler(Looper.getMainLooper())
    private val nsd = context.getSystemService(NsdManager::class.java)
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)
    private val preferences = context.getSharedPreferences("roomwave", Context.MODE_PRIVATE)
    val deviceId: String = preferences.getString("deviceId", null)
        ?: UUID.randomUUID().toString().also {
            // Persist before advertising, and surface storage failure instead of changing identity silently.
            check(preferences.edit().putString("deviceId", it).commit()) { "Cannot persist RoomWave deviceId" }
        }
    val deviceName: String = readableDeviceName()

    var registered by mutableStateOf(false)
        private set
    var localIp by mutableStateOf("No local network")
        private set
    var status by mutableStateOf("Stopped")
        private set
    var error by mutableStateOf<String?>(null)
        private set

    private var active = false
    private var callbackRegistered = false
    private var listener: NsdManager.RegistrationListener? = null
    private var registering = false
    private var unregistering = false
    private val addresses = linkedMapOf<Network, String>()

    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) {
            main.post {
                if (!active) return@post
                val usable = properties.linkAddresses.map { it.address }
                    .filter { !it.isLoopbackAddress && !it.isAnyLocalAddress && !it.isMulticastAddress }
                    .sortedWith(compareBy({ it !is Inet4Address }, { it.hostAddress }))
                val address = usable.firstOrNull()?.hostAddress
                if (address == null) addresses.remove(network) else addresses[network] = address
                networkChanged()
            }
        }

        override fun onLost(network: Network) {
            main.post {
                if (!active) return@post
                addresses.remove(network)
                networkChanged()
            }
        }
    }

    fun start() {
        if (active) return
        active = true
        error = null
        status = "Waiting for Wi-Fi or Ethernet"
        try {
            // Do not require INTERNET capability: a LAN without internet is sufficient.
            val request = NetworkRequest.Builder()
                .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
                .addTransportType(NetworkCapabilities.TRANSPORT_ETHERNET)
                .build()
            connectivity.registerNetworkCallback(request, networkCallback)
            callbackRegistered = true
        } catch (e: RuntimeException) {
            fail("Network monitoring failed", e)
        }
    }

    fun stop() {
        active = false
        if (callbackRegistered) {
            try {
                connectivity.unregisterNetworkCallback(networkCallback)
            } catch (e: RuntimeException) {
                fail("Network monitoring cleanup failed", e)
            }
            callbackRegistered = false
        }
        addresses.clear()
        localIp = "No local network"
        unregister()
    }

    fun retry() {
        error = null
        if (!callbackRegistered) {
            active = false
            start()
        } else if (listener != null) {
            unregister()
        } else {
            networkChanged()
        }
    }

    private fun networkChanged() {
        localIp = addresses.values.distinct().joinToString("\n").ifEmpty { "No local network" }
        if (addresses.isEmpty()) {
            unregister()
            if (listener == null) status = "Waiting for Wi-Fi or Ethernet"
        } else if (listener == null && active && error == null) {
            register()
        }
        // NsdManager follows interface/address changes for an existing registration.
    }

    private fun register() {
        val service = NsdServiceInfo().apply {
            serviceName = "RoomWave-${deviceId.take(8)}"
            // Android appends the .local domain. Rust uses the fully qualified form.
            serviceType = "_roomwave._udp."
            port = 47800
            setAttribute("deviceId", deviceId)
            setAttribute("deviceName", deviceName)
            setAttribute("protocolVersion", "4")
        }
        val registration = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {
                main.post {
                    if (listener !== this) return@post
                    registering = false
                    registered = true
                    status = "Registered (${info.serviceName})"
                    error = null
                    Log.i(TAG, "service registration succeeded: ${info.serviceName}")
                    // onStop may precede this asynchronous callback.
                    if (!active || addresses.isEmpty()) unregister()
                }
            }

            override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                main.post {
                    if (listener !== this) return@post
                    listener = null
                    registering = false
                    registered = false
                    status = "Registration failed"
                    error = "NSD registration error $errorCode. Check Wi-Fi and retry."
                    Log.e(TAG, "service registration failed: code=$errorCode")
                }
            }

            override fun onServiceUnregistered(info: NsdServiceInfo) {
                main.post {
                    if (listener !== this) return@post
                    listener = null
                    unregistering = false
                    registered = false
                    status = "Stopped"
                    Log.i(TAG, "service unregistered: ${info.serviceName}")
                    if (active) networkChanged()
                }
            }

            override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                main.post {
                    if (listener !== this) return@post
                    // Retain listener so Retry can clean up the existing registration.
                    unregistering = false
                    registered = false
                    status = "Unregistration failed"
                    error = "NSD unregistration error $errorCode. Retry or force-stop the app."
                    Log.e(TAG, "service unregistration failed: code=$errorCode")
                }
            }
        }
        listener = registration
        registering = true
        status = "Registering…"
        Log.i(TAG, "service registration started: _roomwave._udp.local. port=47800 id=$deviceId")
        try {
            nsd.registerService(service, NsdManager.PROTOCOL_DNS_SD, registration)
        } catch (e: RuntimeException) {
            listener = null
            registering = false
            fail("Service registration failed", e)
        }
    }

    private fun unregister() {
        registered = false
        val current = listener ?: run { status = "Stopped"; return }
        // Wait for onServiceRegistered before unregistering an in-flight request.
        if (registering || unregistering) return
        unregistering = true
        status = "Unregistering…"
        try {
            nsd.unregisterService(current)
        } catch (e: RuntimeException) {
            unregistering = false
            fail("Service unregistration failed", e)
        }
    }

    private fun fail(message: String, exception: RuntimeException) {
        registered = false
        status = message
        error = "$message: ${exception.message ?: exception.javaClass.simpleName}"
        Log.e(TAG, message, exception)
    }

    private fun readableDeviceName(): String {
        val manufacturer = Build.MANUFACTURER.orEmpty().trim()
        val model = Build.MODEL.orEmpty().trim()
        val name = when {
            model.startsWith(manufacturer, ignoreCase = true) -> model
            else -> "$manufacturer $model".trim()
        }.ifEmpty { "Android device" }.filterNot(Char::isISOControl)
        // Keep the UTF-8 TXT value under 200 bytes, without splitting a surrogate pair.
        return name.codePoints().toArray().fold("") { result, point ->
            val next = result + String(Character.toChars(point))
            if (next.toByteArray(Charsets.UTF_8).size <= 200) next else result
        }.ifEmpty { "Android device" }
    }

    private companion object { const val TAG = "RoomWaveReceiver" }
}
