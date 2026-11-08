// Estados de ejemplo para revisar la interfaz sin servidor ni voz, como las escenas de
// app/src/main.rs: `adb shell am start -n io.github.rubensalas.charlando/.MainActivity --es escena voz-hablando`.
package io.github.rubensalas.charlando

private const val REPLY = "Mira el span de cada etapa: el más largo es tu cuello de botella. Aquí suele ser el ASR."

fun escena(charla: Charla, name: String) {
    val s = charla.estado
    charla.offline()
    charla.settingsOpen = false
    val parts = name.split("-")
    if (parts[0] == "inicio") {
        s.connected = parts.getOrNull(1) != "desconectado"
        return
    }
    val view = if (parts[0] == "voz") View.Voice else View.Chat
    val phase = when (parts.getOrNull(1)) {
        "grabando" -> Phase.Recording
        "pensando" -> Phase.Thinking
        "hablando" -> Phase.Speaking
        else -> Phase.Idle
    }
    s.connected = parts.getOrNull(1) != "desconectado"
    s.screen = Screen.Conversation
    s.view = view
    s.phase = phase
    s.demoLevel = true
    s.messages.clear()
    s.messages.add(Message(Who.You, "¿Qué es un span?"))
    s.messages.add(Message(Who.Claude, "Es un tramo medido de una traza: tiene inicio, fin y atributos. Cada etapa de la cascada sería un span."))
    s.messages.add(Message(Who.You, "¿Y cómo veo cuál etapa es la más lenta?"))
    when (phase) {
        Phase.Idle -> s.messages.add(Message(Who.Claude, REPLY))
        Phase.Speaking -> s.messages.add(Message(Who.Claude, REPLY.take(52)))
        else -> {}
    }
}
