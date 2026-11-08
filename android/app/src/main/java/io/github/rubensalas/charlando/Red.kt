// El WebSocket con el servidor. OkHttp lo lleva en sus propios hilos; aquí solo se reintenta
// cada 2 s mientras no hay conexión (PROTOCOLO.md).
package io.github.rubensalas.charlando

import android.util.Log
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

private const val RETRY_MS = 2_000L
private const val TAG = "charlando"

class Red(
    private val onConnected: (Boolean) -> Unit,
    private val onMessage: (In) -> Unit,
    private val onAudio: (ByteArray) -> Unit,
) : Sender {
    private val client = OkHttpClient.Builder()
        .pingInterval(15, TimeUnit.SECONDS)
        .build()
    private val timer = Executors.newSingleThreadScheduledExecutor { Thread(it, "charlando-red") }
    private val lock = Any()
    private var url = ""
    private var socket: WebSocket? = null
    private var open = false
    /** Cada conexión nueva invalida los avisos que lleguen tarde de la anterior. */
    private var generation = 0
    private var retry: ScheduledFuture<*>? = null
    private var lastError = ""

    /** Cambia la dirección y vuelve a conectar. Vacía deja la app sin conexión. */
    fun connect(newUrl: String) {
        val wasOpen: Boolean
        synchronized(lock) {
            url = newUrl.trim()
            wasOpen = open
            open = false
            generation++
            retry?.cancel(false)
            socket?.cancel()
            socket = null
        }
        if (wasOpen) onConnected(false)
        timer.execute { attempt() }
    }

    fun close() {
        synchronized(lock) {
            generation++
            retry?.cancel(false)
            socket?.cancel()
            socket = null
        }
        timer.shutdownNow()
    }

    override fun send(msg: Out) {
        val ws = synchronized(lock) { socket.takeIf { open } } ?: return
        when (msg) {
            is Out.Audio -> ws.send(msg.pcm.toByteString())
            else -> ws.send(msg.toJson()!!)
        }
    }

    private fun attempt() {
        val gen: Int
        val target: String
        synchronized(lock) {
            if (url.isEmpty() || socket != null) return
            gen = generation
            target = url
        }
        val request = try {
            Request.Builder().url(target).build()
        } catch (e: IllegalArgumentException) {
            fail(gen, "dirección no válida: $target")
            return
        }
        val ws = client.newWebSocket(request, Listener(gen))
        synchronized(lock) {
            if (gen == generation) socket = ws else ws.cancel()
        }
    }

    private fun current(gen: Int) = synchronized(lock) { gen == generation }

    private fun fail(gen: Int, why: String) {
        val wasOpen: Boolean
        synchronized(lock) {
            if (gen != generation) return
            wasOpen = open
            open = false
            socket = null
            generation++
            retry = timer.schedule({ attempt() }, RETRY_MS, TimeUnit.MILLISECONDS)
        }
        if (why != lastError) {
            Log.i(TAG, "sin conexión: $why; reintento cada 2 s")
            lastError = why
        }
        if (wasOpen) onConnected(false)
    }

    private inner class Listener(private val gen: Int) : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            synchronized(lock) {
                if (gen != generation) return
                open = true
            }
            lastError = ""
            Log.i(TAG, "conectado")
            onConnected(true)
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            if (!current(gen)) return
            parse(text)?.let(onMessage)
        }

        override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
            if (!current(gen)) return
            onAudio(bytes.toByteArray())
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            webSocket.close(1000, null)
            fail(gen, "el servidor cerró ($code)")
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            fail(gen, t.toString())
        }
    }
}
