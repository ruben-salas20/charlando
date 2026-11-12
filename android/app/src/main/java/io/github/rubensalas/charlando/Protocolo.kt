// Los mensajes de PROTOCOLO.md, igual que app/src/net.rs.
package io.github.rubensalas.charlando

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** Una carpeta en la que trabaja Claude. `name` es el último tramo de la ruta. */
data class Folder(val path: String, val name: String)

/** De la app al servidor. */
sealed interface Out {
    /**
     * `folder` nulo pide una carpeta nueva (`new_folder`).
     * `resume`: el id de la conversación que se reanuda tras perder la conexión.
     */
    data class Start(
        val folder: String?,
        val model: String,
        val thinking: Boolean,
        val resume: String? = null,
    ) : Out
    data object PttStart : Out
    data object PttEnd : Out
    /** PCM 16 bits LE, mono, 16 kHz. */
    class Audio(val pcm: ByteArray) : Out
    data class PermissionReply(val id: String, val allow: Boolean) : Out

    /** El JSON de los mensajes de texto; `null` para el audio. */
    fun toJson(): String? = when (this) {
        is Start -> buildJsonObject {
            put("type", "start")
            if (folder != null) put("folder", folder) else put("new_folder", true)
            put("model", model)
            put("thinking", thinking)
            resume?.let { put("resume", it) }
        }.toString()
        PttStart -> """{"type":"ptt_start"}"""
        PttEnd -> """{"type":"ptt_end"}"""
        is Audio -> null
        is PermissionReply -> buildJsonObject {
            put("type", "permission_reply")
            put("id", id)
            put("allow", allow)
        }.toString()
    }
}

/** Del servidor a la app (los mensajes de texto; el audio va directo al reproductor). */
sealed interface In {
    /** Las carpetas recientes, la más reciente primero. */
    data class Hello(val folders: List<Folder>) : In
    /** `session`: el id de la conversación, para reanudarla. `folder`: dónde trabaja Claude. */
    data class Ready(val session: String, val folder: Folder) : In
    data class State(val state: String) : In
    data class Transcript(val text: String) : In
    data class Sentence(val text: String) : In
    data object ReplyEnd : In
    /** `detail` se muestra tal cual. */
    data class Permission(val id: String, val tool: String, val detail: String) : In
    data class Error(val message: String) : In
    /** La conversación que se quería reanudar ya no existe. */
    data object SessionGone : In
    /** La carpeta de `start` no existe. */
    data class FolderMissing(val message: String) : In
}

private fun JsonObject.str(k: String) = (this[k] as? JsonPrimitive)?.contentOrNull ?: ""

fun parse(text: String): In? {
    val obj = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: return null
    fun s(k: String) = obj.str(k)
    return when ((obj["type"] as? JsonPrimitive)?.jsonPrimitive?.contentOrNull) {
        "hello" -> In.Hello(
            (obj["folders"] as? JsonArray).orEmpty()
                .mapNotNull { it as? JsonObject }
                .map { Folder(it.str("path"), it.str("name")) }
                .filter { it.path.isNotEmpty() },
        )
        "ready" -> In.Ready(s("session"), Folder(s("folder"), s("name")))
        "state" -> In.State(s("state"))
        "transcript" -> In.Transcript(s("text"))
        "sentence" -> In.Sentence(s("text"))
        "reply_end" -> In.ReplyEnd
        "permission" -> In.Permission(s("id"), s("tool"), s("detail"))
        "error" -> when (s("code")) {
            "session_gone" -> In.SessionGone
            "folder_missing" -> In.FolderMissing(s("message"))
            else -> In.Error(s("message"))
        }
        else -> null
    }
}
