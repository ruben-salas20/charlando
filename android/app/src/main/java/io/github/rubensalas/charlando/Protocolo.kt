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
    data class Start(val model: String, val thinking: Boolean) : Out
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
        }.toString()
        PttStart -> """{"type":"ptt_start"}"""
        PttEnd -> """{"type":"ptt_end"}"""
        is Audio -> null
    }
}

/** Del servidor a la app (los mensajes de texto; el audio va directo al reproductor). */
sealed interface In {
    data class Ready(val model: String) : In
    data class State(val state: String) : In
    data class Transcript(val text: String) : In
    data class Sentence(val text: String) : In
    data object ReplyEnd : In
    data class Error(val message: String) : In
}

fun parse(text: String): In? {
    val obj = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: return null
    fun s(k: String) = (obj[k] as? JsonPrimitive)?.contentOrNull ?: ""
    return when ((obj["type"] as? JsonPrimitive)?.jsonPrimitive?.contentOrNull) {
        "ready" -> In.Ready(s("model"))
        "state" -> In.State(s("state"))
        "transcript" -> In.Transcript(s("text"))
        "sentence" -> In.Sentence(s("text"))
        "reply_end" -> In.ReplyEnd
        "error" -> In.Error(s("message"))
        else -> null
    }
}
