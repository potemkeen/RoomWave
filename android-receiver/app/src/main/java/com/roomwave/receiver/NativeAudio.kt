package com.roomwave.receiver

/** JNI calls run on network/control workers; the AAudio callback never enters the JVM. */
object NativeAudio {
    init { System.loadLibrary("roomwave_audio") }
    external fun open(): Long
    external fun clock(handle: Long, offset: Long, at: Long)
    external fun push(handle: Long, frame: Long, capture: Long, send: Long, play: Long, receive: Long, pcm: ByteArray): Boolean
    external fun poll(handle: Long): DoubleArray
    external fun close(handle: Long)
}
