// Une el estado con la red, el audio y los ajustes. Vive en un ViewModel para que girar la
// pantalla no corte la conexión.
package io.github.rubensalas.charlando

import android.app.Application
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class Prefs(context: Context) : Store {
    private val sp = context.getSharedPreferences("charlando", Context.MODE_PRIVATE)

    var url: String
        get() = sp.getString("url", "") ?: ""
        set(v) = sp.edit().putString("url", v).apply()

    override fun loadView() = if (sp.getString("view", "chat") == "voz") View.Voice else View.Chat
    override fun saveView(view: View) = sp.edit().putString("view", if (view == View.Voice) "voz" else "chat").apply()
    override fun loadThinking() = sp.getBoolean("thinking", false)
    override fun saveThinking(thinking: Boolean) = sp.edit().putBoolean("thinking", thinking).apply()
}

class Charla(app: Application) : AndroidViewModel(app) {
    private val prefs = Prefs(app)
    private val main = Handler(Looper.getMainLooper())
    private val player = AndroidPlayer()
    private val red = Red(
        onConnected = { c -> main.post { estado.onConnected(c) } },
        onTaken = { main.post { estado.onTaken() } },
        onMessage = { m -> main.post { estado.onMessage(m) } },
        onAudio = { pcm -> player.pushPcm16(pcm) },
    )
    private val mic = AndroidMic()
    val estado: Estado = Estado(red, mic, player, prefs, reconnect = red::reconnect)

    var url by mutableStateOf(prefs.url)
        private set
    /** La primera vez no hay dirección: se abren los ajustes solos. */
    var settingsOpen by mutableStateOf(url.isEmpty())
    /** La hoja para elegir carpeta en el inicio. */
    var foldersOpen by mutableStateOf(false)

    init {
        red.connect(url)
        viewModelScope.launch {
            val t0 = SystemClock.elapsedRealtime()
            while (true) {
                estado.tick((SystemClock.elapsedRealtime() - t0) / 1000.0)
                delay(33)
            }
        }
    }

    /** Para las escenas de prueba: sin red, nada las devuelve al inicio. */
    fun offline() {
        red.connect("")
    }

    fun saveUrl(newUrl: String) {
        val u = newUrl.trim()
        settingsOpen = false
        if (u == url) return
        url = u
        prefs.url = u
        red.connect(u)
    }

    override fun onCleared() {
        red.close()
        mic.cancel()
        player.release()
    }
}
