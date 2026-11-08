// Micrófono a 16 kHz para el servidor, altavoz a 24 kHz para la respuesta, y el nivel RMS de
// ambos para la bola y el medidor. Las cuentas de nivel son las de app/src/audio.rs.
package io.github.rubensalas.charlando

import android.annotation.SuppressLint
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.util.Log
import java.util.ArrayDeque
import kotlin.concurrent.thread
import kotlin.math.exp
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.sqrt

const val SEND_RATE = 16_000
const val REPLY_RATE = 24_000
/** 40 ms a 16 kHz: dentro de los 20–100 ms que pide el protocolo. */
private const val CHUNK = 640
private const val TAG = "charlando"

fun rms(samples: ShortArray, n: Int = samples.size): Float {
    if (n == 0) return 0f
    var sum = 0.0
    for (i in 0 until n) {
        val s = samples[i] / 32768.0
        sum += s * s
    }
    return sqrt(sum / n).toFloat()
}

/** RMS a 0..1: -50 dBFS es 0 y -10 dBFS es 1, el rango de una voz normal al micrófono. */
fun meter(rms: Float): Float {
    val db = 20f * log10(max(rms, 1e-6f))
    return ((db + 50f) / 40f).coerceIn(0f, 1f)
}

/** Sube rápido y baja despacio, como un vúmetro. */
class Level {
    @Volatile var value = 0f
        private set

    fun publish(next: Float, blockSecs: Float) {
        val old = value
        val fall = exp(-blockSecs / 0.25f)
        value = if (next > old) old + (next - old) * 0.6f else next + (old - next) * fall
    }

    fun reset() {
        value = 0f
    }
}

class NullMic : Mic {
    override fun begin(net: Sender, start: Out) = net.send(start)
    override fun end(net: Sender, end: Out) = net.send(end)
    override fun cancel() {}
    override fun level() = 0f
}

class NullPlayer : Player {
    override fun clear() {}
    override fun isEmpty() = true
    override fun level() = 0f
}

/**
 * El micrófono solo se abre mientras se graba, para que Android no muestre el aviso de
 * micrófono en uso durante toda la conversación.
 */
class AndroidMic : Mic {
    private val lock = Any()
    private var net: Sender? = null
    private var recording = false
    private var generation = 0
    private val level = Level()

    @SuppressLint("MissingPermission")
    override fun begin(net: Sender, start: Out) {
        val gen: Int
        synchronized(lock) {
            net.send(start)
            this.net = net
            recording = true
            gen = ++generation
        }
        thread(name = "charlando-micro") { capture(gen) }
    }

    override fun end(net: Sender, end: Out) {
        synchronized(lock) {
            recording = false
            net.send(end)
        }
    }

    override fun cancel() {
        synchronized(lock) { recording = false }
    }

    override fun level() = level.value

    private fun active(gen: Int) = synchronized(lock) { recording && generation == gen }

    @SuppressLint("MissingPermission")
    private fun capture(gen: Int) {
        val min = AudioRecord.getMinBufferSize(SEND_RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val rec = try {
            AudioRecord(
                MediaRecorder.AudioSource.VOICE_COMMUNICATION, SEND_RATE,
                AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, max(min, CHUNK * 2 * 4),
            )
        } catch (e: Exception) {
            Log.w(TAG, "micrófono: $e")
            return
        }
        if (rec.state != AudioRecord.STATE_INITIALIZED) {
            Log.w(TAG, "micrófono: no se pudo abrir")
            rec.release()
            return
        }
        val buf = ShortArray(CHUNK)
        try {
            rec.startRecording()
            while (active(gen)) {
                var n = 0
                while (n < CHUNK) {
                    val r = rec.read(buf, n, CHUNK - n)
                    if (r <= 0) break
                    n += r
                }
                if (n <= 0) break
                level.publish(meter(rms(buf, n)), n.toFloat() / SEND_RATE)
                val bytes = ByteArray(n * 2)
                for (i in 0 until n) {
                    bytes[2 * i] = (buf[i].toInt() and 0xff).toByte()
                    bytes[2 * i + 1] = (buf[i].toInt() shr 8).toByte()
                }
                // Con el cerrojo, para que ningún trozo salga después de `ptt_end`.
                synchronized(lock) {
                    if (recording && generation == gen) net?.send(Out.Audio(bytes))
                }
            }
        } finally {
            rec.stop()
            rec.release()
            level.reset()
        }
    }
}

/**
 * Reproduce en streaming lo que llega a 24 kHz. Un hilo propio saca trozos de la cola y los
 * escribe en el AudioTrack; `isEmpty` mira también lo que el altavoz aún no ha sonado.
 */
class AndroidPlayer : Player {
    // wait/notify necesitan un Object de Java.
    @Suppress("PLATFORM_CLASS_MAPPED_TO_KOTLIN")
    private val lock = Object()
    private val queue = ArrayDeque<ShortArray>()
    private var odd: Byte? = null
    private var clearRequested = false
    /** Muestras escritas en el AudioTrack desde el último vaciado. */
    @Volatile private var written = 0L
    @Volatile private var stopped = false
    private val level = Level()
    private val track: AudioTrack

    init {
        val min = AudioTrack.getMinBufferSize(REPLY_RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT)
        track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setSampleRate(REPLY_RATE)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .build(),
            )
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setBufferSizeInBytes(max(min * 2, REPLY_RATE / 5 * 2))
            .build()
        // Por defecto empieza a sonar con el búfer lleno: el final de una respuesta corta
        // se quedaría sin sonar y la app no volvería nunca a «En espera».
        track.setStartThresholdInFrames(480)
        track.play()
        thread(name = "charlando-altavoz", isDaemon = true) { loop() }
    }

    /** PCM 16 bits LE mono a 24 kHz, en trozos de cualquier tamaño. */
    fun pushPcm16(bytes: ByteArray) {
        synchronized(lock) {
            val data = if (odd != null) byteArrayOf(odd!!) + bytes else bytes
            odd = if (data.size % 2 == 1) data.last() else null
            val n = data.size / 2
            if (n == 0) return
            val samples = ShortArray(n) { i ->
                ((data[2 * i].toInt() and 0xff) or (data[2 * i + 1].toInt() shl 8)).toShort()
            }
            queue.addLast(samples)
            lock.notifyAll()
        }
    }

    override fun clear() {
        synchronized(lock) {
            queue.clear()
            odd = null
            clearRequested = true
            lock.notifyAll()
        }
        level.reset()
    }

    override fun isEmpty(): Boolean = synchronized(lock) {
        queue.isEmpty() && (clearRequested || track.playbackHeadPosition.toLong() >= written)
    }

    override fun level() = level.value

    fun release() {
        stopped = true
    }

    private fun loop() {
        val slice = REPLY_RATE / 50
        while (!stopped) {
            var chunk: ShortArray? = null
            synchronized(lock) {
                if (clearRequested) {
                    track.pause()
                    track.flush()
                    track.play()
                    written = 0
                    clearRequested = false
                }
                chunk = queue.pollFirst()
                if (chunk == null) lock.wait(20)
            }
            val c = chunk
            if (c == null) {
                level.publish(0f, 0.02f)
                continue
            }
            // Trozos de 20 ms: el nivel sigue a la voz y `clear` no espera a un trozo largo.
            var off = 0
            while (off < c.size) {
                if (synchronized(lock) { clearRequested }) break
                val n = minOf(slice, c.size - off)
                level.publish(meter(rms(c.copyOfRange(off, off + n))), n.toFloat() / REPLY_RATE)
                val w = track.write(c, off, n, AudioTrack.WRITE_BLOCKING)
                if (w <= 0) break
                synchronized(lock) { if (!clearRequested) written += w }
                off += w
            }
        }
        track.stop()
        track.release()
    }
}
