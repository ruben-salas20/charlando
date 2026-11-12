// Estados de ejemplo para revisar la interfaz sin servidor ni voz, como las escenas de
// app/src/main.rs: `adb shell am start -n io.github.rubensalas.charlando/.MainActivity --es escena voz-hablando`.
package io.github.rubensalas.charlando

private const val LONG_QUESTION = "Hola, quiero que me ayudes a pensar en cómo escribir un post para X, donde la idea es comentar un poco que estoy aprendiendo sobre un agente de voz desarrollado en tipo cascada, o sea que es un modelo ASR que pasa a un LLM, que es el que piensa y luego pasa a un text-to-speech, que es el que genera la voz."
private const val LONG_REPLY = "Sí, se entiende bien, y la idea tiene mucho sentido para un post. Lo que describes es un pipeline en cascada: el audio entra por un ASR, el texto va a un LLM que razona y responde, y esa respuesta pasa a un TTS que la convierte en voz. Construirlo tú mismo en Rust, con un LLM intercambiable, es un buen ángulo porque muestra que no hace falta magia: son tres piezas conectadas y el reto está en la latencia y en cómo se pasan el turno."
private const val REPLY = "Mira el span de cada etapa: el más largo es tu cuello de botella. Aquí suele ser el ASR."
private val FOLDERS = listOf(
    Folder("~/proyectos/mi-proyecto", "mi-proyecto"),
    Folder("~/notas", "notas"),
    Folder("~/trabajo/cliente", "trabajo"),
)
private val PERMISSION = In.Permission("p1", "Bash", "git push origin main")

/**
 * Escenas: `inicio`, `inicio-desconectado`, `inicio-carpetas` (con recientes), `inicio-hoja` (hoja de
 * carpetas abierta), `inicio-falta` (carpeta borrada), y `chat-` o `voz-` con `espera`, `grabando`,
 * `pensando`, `hablando`, `larga`, `desconectado` o `permiso`.
 */
fun escena(charla: Charla, name: String) {
    val s = charla.estado
    charla.offline()
    charla.settingsOpen = false
    val parts = name.split("-")
    if (parts[0] == "inicio") {
        s.connected = parts.getOrNull(1) != "desconectado"
        if (parts.getOrNull(1) in listOf("carpetas", "hoja", "falta")) s.onMessage(In.Hello(FOLDERS, "~/charlando"))
        charla.foldersOpen = parts.getOrNull(1) == "hoja"
        if (parts.getOrNull(1) == "falta") s.onMessage(In.FolderMissing("La carpeta ~/proyectos/mi-proyecto ya no existe."))
        return
    }
    val view = if (parts[0] == "voz") View.Voice else View.Chat
    val phase = when (parts.getOrNull(1)) {
        "grabando" -> Phase.Recording
        "pensando" -> Phase.Thinking
        "hablando", "larga" -> Phase.Speaking
        "permiso" -> Phase.Permission
        else -> Phase.Idle
    }
    s.connected = parts.getOrNull(1) != "desconectado"
    s.screen = Screen.Conversation
    s.current = FOLDERS[0]
    s.permission = if (phase == Phase.Permission) PERMISSION else null
    s.view = view
    s.phase = phase
    s.demoLevel = true
    s.messages.clear()
    s.messages.add(Message(Who.You, "¿Qué es un span?"))
    s.messages.add(Message(Who.Claude, "Es un tramo medido de una traza: tiene inicio, fin y atributos. Cada etapa de la cascada sería un span."))
    s.messages.add(Message(Who.You, "¿Y cómo veo cuál etapa es la más lenta?"))
    if (parts.getOrNull(1) == "larga") {
        s.messages[2] = Message(Who.You, LONG_QUESTION)
        s.messages.add(Message(Who.Claude, LONG_REPLY))
        return
    }
    when (phase) {
        Phase.Idle, Phase.Permission -> s.messages.add(Message(Who.Claude, REPLY))
        Phase.Speaking -> s.messages.add(Message(Who.Claude, REPLY.take(52)))
        else -> {}
    }
}
