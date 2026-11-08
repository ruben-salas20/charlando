// Los tests de app/src/main.rs y app/src/net.rs, más los del toque y el turno con herramienta.
package io.github.rubensalas.charlando

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FakePlayer : Player {
    var empty = true
    var cleared = 0
    override fun clear() { cleared++; empty = true }
    override fun isEmpty() = empty
    override fun level() = 0f
}

class MemoryStore : Store {
    var view = View.Chat
    var thinking = false
    override fun loadView() = view
    override fun saveView(view: View) { this.view = view }
    override fun loadThinking() = thinking
    override fun saveThinking(thinking: Boolean) { this.thinking = thinking }
}

class EstadoTest {
    private val sent = mutableListOf<Out>()
    private val player = FakePlayer()
    private val store = MemoryStore()

    private fun connected(): Estado {
        val s = Estado({ sent.add(it) }, NullMic(), player, store)
        s.onConnected(true)
        s.start()
        s.onMessage(In.Ready("haiku"))
        return s
    }

    @Test
    fun aTurnWalksTheProtocolStates() {
        val s = connected()
        assertEquals(Screen.Conversation, s.screen)
        s.pttDown()
        assertEquals(Phase.Recording, s.phase)
        s.pttUp()
        assertEquals(Phase.Thinking, s.phase)
        s.onMessage(In.State("thinking"))
        s.onMessage(In.Transcript("Hola"))
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Hola."))
        s.onMessage(In.Sentence("¿Qué tal?"))
        assertEquals(Phase.Speaking, s.phase)
        assertEquals("Hola. ¿Qué tal?", s.lastReply())
        s.onMessage(In.ReplyEnd)
        assertTrue(s.tick(0.0))
        assertEquals(Phase.Idle, s.phase)
        assertEquals(listOf<Out>(Out.Start("haiku", false), Out.PttStart, Out.PttEnd), sent)
    }

    @Test
    fun aToolCallThinksMidTurnAndKeepsOneBubble() {
        val s = connected()
        s.pttDown()
        s.pttUp()
        s.onMessage(In.Transcript("Busca algo"))
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Déjame mirarlo."))
        s.onMessage(In.State("thinking"))
        assertEquals("sigue hablando hasta vaciar la cola", Phase.Speaking, s.phase)
        s.tick(0.0)
        assertEquals(Phase.Thinking, s.phase)
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Ya lo tengo."))
        assertEquals(Phase.Speaking, s.phase)
        assertEquals("Déjame mirarlo. Ya lo tengo.", s.lastReply())
        assertEquals(1, s.messages.count { it.who == Who.Claude })
    }

    /** speaking → thinking → speaking con audio aún en cola: cada paso espera a que se vacíe. */
    @Test
    fun aToolTurnWaitsForTheQueueBeforeThinkingAndBeforeIdle() {
        val s = connected()
        s.tap()
        s.tap()
        s.onMessage(In.Transcript("¿Qué hay en la carpeta?"))
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Déjame mirarlo."))
        player.empty = false
        s.onMessage(In.State("thinking"))
        s.tick(0.0)
        assertEquals("aún suena «Déjame mirarlo.»", Phase.Speaking, s.phase)
        s.tap()
        assertEquals("tocar mientras habla no hace nada", Phase.Speaking, s.phase)
        player.empty = true
        s.tick(0.0)
        assertEquals(Phase.Thinking, s.phase)
        s.tap()
        assertEquals("tocar mientras piensa no hace nada", Phase.Thinking, s.phase)
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Hay dos archivos."))
        player.empty = false
        s.onMessage(In.ReplyEnd)
        s.tick(0.0)
        assertEquals("no vuelve a En espera con audio en cola", Phase.Speaking, s.phase)
        player.empty = true
        s.tick(0.0)
        assertEquals(Phase.Idle, s.phase)
        assertEquals(listOf("¿Qué hay en la carpeta?", "Déjame mirarlo. Hay dos archivos."), s.messages.map { it.text })
        assertEquals(1, sent.count { it == Out.PttStart })
    }

    @Test
    fun silenceGoesBackToIdleWithoutAReply() {
        val s = connected()
        s.pttDown()
        s.pttUp()
        s.onMessage(In.Transcript(""))
        s.onMessage(In.ReplyEnd)
        s.tick(0.0)
        assertEquals(Phase.Idle, s.phase)
        assertTrue(s.messages.isEmpty())
    }

    @Test
    fun tapsAreIgnoredWhileClaudeThinksOrIsDisconnected() {
        val s = connected()
        s.tap()
        s.tap()
        s.tap()
        assertEquals(Phase.Thinking, s.phase)
        s.onConnected(false)
        s.tap()
        assertEquals(Phase.Idle, s.phase)
        s.onConnected(true)
        assertEquals("al reconectar vuelve al inicio", Screen.Start, s.screen)
    }

    @Test
    fun anErrorEndsTheTurn() {
        val s = connected()
        s.tap()
        s.tap()
        s.onMessage(In.Error("ASR caído"))
        s.tick(0.0)
        assertEquals(Phase.Idle, s.phase)
        assertEquals("ASR caído", s.error)
        s.tap()
        assertNull("al hablar otra vez se borra el error", s.error)
    }

    @Test
    fun plusGoesBackToStartAndThePreferencesAreSaved() {
        val s = connected()
        s.changeView(View.Voice)
        s.toggleThinking()
        assertEquals(View.Voice, store.view)
        assertTrue(store.thinking)
        s.newConversation()
        assertEquals(Screen.Start, s.screen)
        assertTrue(s.messages.isEmpty())
        s.start()
        assertEquals(Out.Start("haiku", true), sent.last())
    }
}

class ProtocoloTest {
    @Test
    fun parsesEveryServerMessage() {
        assertEquals(In.Ready("haiku"), parse("""{"type":"ready","model":"haiku"}"""))
        assertEquals(In.State("speaking"), parse("""{"type":"state","state":"speaking"}"""))
        assertEquals(In.Transcript(""), parse("""{"type":"transcript","text":""}"""))
        assertEquals(In.Sentence("Hola."), parse("""{"type":"sentence","text":"Hola."}"""))
        assertEquals(In.ReplyEnd, parse("""{"type":"reply_end"}"""))
        assertEquals(In.Error("x"), parse("""{"type":"error","message":"x"}"""))
        assertNull(parse("""{"type":"otro"}"""))
        assertNull(parse("no es json"))
    }

    @Test
    fun messagesFollowTheProtocol() {
        assertEquals("""{"type":"start","model":"opus","thinking":true}""", Out.Start("opus", true).toJson())
        assertEquals("""{"type":"ptt_start"}""", Out.PttStart.toJson())
        assertEquals("""{"type":"ptt_end"}""", Out.PttEnd.toJson())
    }

    @Test
    fun meterMapsSpeechLevels() {
        assertEquals(0f, meter(0f), 1e-6f)
        assertEquals(1f, meter(1f), 1e-6f)
        val m = meter(0.03f)
        assertTrue("$m", m > 0.3f && m < 0.9f)
    }
}
