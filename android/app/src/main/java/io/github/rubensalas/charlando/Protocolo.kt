// Los mensajes de PROTOCOLO.md, igual que app/src/net.rs.
package io.github.rubensalas.charlando

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** De la app al servidor. */
sealed interface Out {
    /** `resume`: el id de la conversación que se reanuda tras perder la conexión. */
    data class Start(val model: String, val thinking: Boolean, val resume: String? = null) : Out
    data object PttStart : Out
    data object PttEnd : Out
    /** PCM 16 bits LE, mono, 16 kHz. */
    class Audio(val pcm: ByteArray) : Out

    /** El JSON de los mensajes de texto; `null` para el audio. */
    fun toJson(): String? = when (this) {
        is Start -> buildJsonObject {
            put("type", "start")
            put("model", model)
            put("thinking", thinking)
            resume?.let { put("resume", it) }
        }.toString()
        PttStart -> """{"type":"ptt_start"}"""
        PttEnd -> """{"type":"ptt_end"}"""
        is Audio -> null
    }
}

/** Del servidor a la app (los mensajes de texto; el audio va directo al reproductor). */
sealed interface In {
    /** `session`: el id de la conversación, para reanudarla. */
    data class Ready(val session: String) : In
    data class State(val state: String) : In
    data class Transcript(val text: String) : In
    data class Sentence(val text: String) : In
    data object ReplyEnd : In
    data class Error(val message: String) : In
    /** La conversación que se quería reanudar ya no existe. */
    data object SessionGone : In
}

fun parse(text: String): In? {
    val obj = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: return null
    fun s(k: String) = (obj[k] as? JsonPrimitive)?.contentOrNull ?: ""
    return when ((obj["type"] as? JsonPrimitive)?.jsonPrimitive?.contentOrNull) {
        "ready" -> In.Ready(s("session"))
        "state" -> In.State(s("state"))
        "transcript" -> In.Transcript(s("text"))
        "sentence" -> In.Sentence(s("text"))
        "reply_end" -> In.ReplyEnd
        "error" -> if (s("code") == "session_gone") In.SessionGone else In.Error(s("message"))
        else -> null
    }
}
