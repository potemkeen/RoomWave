package com.roomwave.receiver

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.roundToInt

/** Stateless causal interpolation. 21 prefix samples carry FIR history across losses/reorder. */
object Pcm16k {
    private val taps=DoubleArray(63) { i ->
        val t=i-31.0; val f=6500.0/48000
        (if(t==0.0) 2*f else sin(2*PI*f*t)/(PI*t))*(0.54-0.46*cos(2*PI*i/62))
    }.let { h -> val sum=h.sum(); DoubleArray(63) { h[it]*3/sum } }
    fun expand(bytes: ByteArray, channels: Int, out: ByteArray = ByteArray(960)): ByteArray {
        require(out.size == 960)
        fun sample(n: Int,c: Int): Int {
            val k=64+((n+21)*channels+c)*2
            return ((bytes[k].toInt() and 255) or (bytes[k+1].toInt() shl 8)).toShort().toInt()
        }
        for(n in 0 until 240) for(c in 0 until channels) {
            var value=0.0
            for(t in n%3 until 63 step 3) value+=taps[t]*sample((n-t)/3,c)
            val pcm=value.roundToInt().coerceIn(-32768,32767)
            val k=(n*2+c)*2;out[k]=pcm.toByte();out[k+1]=(pcm shr 8).toByte()
            if(channels==1) {out[k+2]=out[k];out[k+3]=out[k+1]}
        }
        return out
    }
}
