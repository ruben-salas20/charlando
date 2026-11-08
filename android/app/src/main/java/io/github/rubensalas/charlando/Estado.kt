// El estado de la app y cómo cambia con lo que llega del servidor.
// Es una traducción de app/src/state.rs: los dos clientes deben comportarse igual.
package io.github.rubensalas.charlando

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlin.math.abs
import kotlin.math.min
import kotlin.math.sin

enum class Model(val id: String, val title: String, val blurb: String) {
    Haiku("haiku", "Claude Haiku 5.5", "El más rápido"),
    Sonnet("sonnet", "Claude Sonnet 5.5", "Equilibrado"),
    Opus("opus", "Claude Opus 5.5", "El más capaz, más lento"),
}

enum class Screen { Start, Conversation }
enum class View { Chat, Voice }
enum class Phase { Idle, Recording, Thinking, Speaking }
enum class Who { You, Claude }

data class Message(val who: Who, val text: String)

fun interface Sender {
    fun send(msg: Out)
}

interface Mic {
    /** Envía `start` y empieza a mandar audio; ningún trozo se adelanta al mensaje. */
    fun begin(net: Sender, start: Out)
    /** Manda lo que quede y después `end`. */
    fun end(net: Sender, end: Out)
    fun cancel()
    fun level(): Float
}

interface Player {
    fun clear()
    /** No queda nada por sonar: ni en la cola ni en el búfer del altavoz. */
    fun isEmpty(): Boolean
    fun level(): Float
}

/** Lo que se recuerda entre sesiones. */
interface Store {
    fun loadView(): View
    fun saveView(view: View)
    fun loadThinking(): Boolean
    fun saveThinking(thinking: Boolean)
}

class Estado(
    private val net: Sender?,
    private val mic: Mic,
    private val player: Player,
    private val store: Store,
) {
    var screen by mutableStateOf(Screen.Start)
    var view by mutableStateOf(store.loadView())
        internal set
    var model by mutableStateOf(Model.Haiku)
    /** Interruptor «Razonar»: Claude decide cuándo razonar. */
    var thinking by mutableStateOf(store.loadThinking())
        private set
    var connected by mutableStateOf(false)
    /** Se pulsó «Empezar» y esperamos `ready`. */
    var starting by mutableStateOf(false)
    var phase by mutableStateOf(Phase.Idle)
    val messages = mutableStateListOf<Message>()
    /** Ya llegó `reply_end`: falta que se vacíe la cola de reproducción. */
    var replyEnded = false
    /** Ya hay burbuja de Claude en este turno: lo que llegue tras una herramienta va a la misma. */
    var replyStarted = false
    /** Llegó `thinking` mientras sonaba la voz: se pasa a Pensando cuando termine de sonar. */
    var pendingThinking = false
    var error by mutableStateOf<String?>(null)
    /** Nivel de audio 0..1 que se dibuja (micrófono o voz de Claude). */
    var level by mutableFloatStateOf(0f)
    /** En las capturas de prueba el nivel sale de la misma función que la maqueta. */
    var demoLevel = false

    private fun send(msg: Out) {
        net?.send(msg)
    }

    fun start() {
        if (!connected || starting) return
        starting = true
        send(Out.Start(model.id, thinking))
    }

    fun toggleThinking() {
        thinking = !thinking
        store.saveThinking(thinking)
    }

    fun newConversation() {
        mic.cancel()
        player.clear()
        screen = Screen.Start
        starting = false
        phase = Phase.Idle
        messages.clear()
        replyEnded = false
        error = null
    }

    fun changeView(v: View) {
        if (view != v) {
            view = v
            store.saveView(v)
        }
    }

    fun canTalk() = connected && screen == Screen.Conversation && phase == Phase.Idle

    fun pttDown() {
        if (!canTalk()) return
        error = null
        phase = Phase.Recording
        net?.let { mic.begin(it, Out.PttStart) }
    }

    fun pttUp() {
        if (phase != Phase.Recording) return
        phase = Phase.Thinking
        replyEnded = false
        replyStarted = false
        pendingThinking = false
        net?.let { mic.end(it, Out.PttEnd) }
    }

    /** Tocar para hablar: un toque graba y otro envía. En Pensando y Hablando no hace nada. */
    fun tap() {
        if (phase == Phase.Recording) pttUp() else pttDown()
    }

    fun onConnected(isConnected: Boolean) {
        connected = isConnected
        mic.cancel()
        player.clear()
        starting = false
        phase = Phase.Idle
        replyEnded = false
        if (isConnected) {
            // PROTOCOLO: al reconectar, vuelve a la pantalla de inicio.
            screen = Screen.Start
            messages.clear()
            error = null
        }
    }

    fun onMessage(msg: In) {
        when (msg) {
            is In.Ready -> if (starting) {
                starting = false
                screen = Screen.Conversation
                phase = Phase.Idle
                messages.clear()
                error = null
            }
            is In.State -> when (msg.state) {
                "thinking" -> when (phase) {
                    Phase.Speaking -> pendingThinking = true
                    Phase.Recording -> {}
                    else -> phase = Phase.Thinking
                }
                "speaking" -> {
                    phase = Phase.Speaking
                    pendingThinking = false
                    if (!replyStarted) {
                        replyStarted = true
                        messages.add(Message(Who.Claude, ""))
                    }
                }
            }
            is In.Transcript -> if (msg.text.isNotBlank()) messages.add(Message(Who.You, msg.text))
            is In.Sentence -> {
                val last = messages.lastOrNull()
                val text = msg.text.trim()
                if (last != null && last.who == Who.Claude && replyStarted) {
                    val joined = if (last.text.isEmpty()) text else "${last.text} $text"
                    messages[messages.lastIndex] = last.copy(text = joined)
                } else {
                    messages.add(Message(Who.Claude, text))
                }
            }
            In.ReplyEnd -> replyEnded = true
            is In.Error -> {
                error = msg.message
                replyEnded = true
            }
        }
    }

    /** Cada fotograma: nivel de audio y fin de la reproducción. True si cambió algo visible. */
    fun tick(t: Double): Boolean {
        var changed = false
        if (replyEnded && (phase == Phase.Thinking || phase == Phase.Speaking) && player.isEmpty()) {
            phase = Phase.Idle
            replyEnded = false
            pendingThinking = false
            changed = true
        } else if (pendingThinking && phase == Phase.Speaking && player.isEmpty()) {
            phase = Phase.Thinking
            pendingThinking = false
            changed = true
        }
        val next = when {
            demoLevel -> demoLevel(t)
            phase == Phase.Recording -> mic.level()
            phase == Phase.Speaking -> player.level()
            else -> 0f
        }
        if (abs(next - level) > 0.01f) {
            level = next
            changed = true
        }
        return changed
    }

    fun lastQuestion(): String? = messages.lastOrNull { it.who == Who.You }?.text

    /** La respuesta que va con la última pregunta (o la última si no hubo pregunta). */
    fun lastReply(): String? = messages.lastOrNull()?.takeIf { it.who == Who.Claude }?.text
}

/** La función `level()` de la maqueta, con `t` en segundos. */
fun demoLevel(t: Double): Float {
    val now = t * 1000.0
    return (min(abs(sin(now / 170.0) * sin(now / 410.0)), 1.0 / 1.4) * 1.4).toFloat()
}
