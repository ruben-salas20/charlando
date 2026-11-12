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

    private val proyecto = Folder("/p/proyecto", "proyecto")

    private fun connected(): Estado {
        val s = Estado({ sent.add(it) }, NullMic(), player, store)
        s.onConnected(true)
        s.onMessage(In.Hello(listOf(proyecto)))
        s.start()
        s.onMessage(In.Ready("s1", proyecto))
        return s
    }

    /** Un turno hasta que Claude pide permiso y termina de sonar el aviso. */
    private fun askPermission(s: Estado) {
        s.tap()
        s.tap()
        s.onMessage(In.Transcript("Sube los cambios"))
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Necesito tu permiso. Míralo en pantalla."))
        s.onMessage(In.Permission("p1", "Bash", "git push origin main"))
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
        assertEquals(listOf<Out>(Out.Start("/p/proyecto", "haiku", false), Out.PttStart, Out.PttEnd), sent)
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
        assertEquals(Screen.Conversation, s.screen)
    }

    @Test
    fun reconnectingResumesTheConversation() {
        val s = connected()
        s.tap()
        s.tap()
        s.onMessage(In.Transcript("Hola"))
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Hola, ¿qué tal?"))
        s.onConnected(false)
        s.onConnected(true)
        assertEquals(Screen.Conversation, s.screen)
        assertEquals(Out.Start("/p/proyecto", "haiku", false, "s1"), sent.last())
        assertTrue("no se habla hasta que llegue ready", !s.canTalk())
        s.onMessage(In.Ready("s1", proyecto))
        assertTrue(s.canTalk())
        assertEquals("la conversación sigue ahí", 2, s.messages.size)
    }

    @Test
    fun aLostSessionGoesBackToStart() {
        val s = connected()
        s.messages.add(Message(Who.You, "Hola"))
        s.onConnected(false)
        s.onConnected(true)
        s.onMessage(In.SessionGone)
        assertEquals(Screen.Start, s.screen)
        assertTrue(s.messages.isEmpty())
        assertNull(s.session)
    }

    @Test
    fun reconnectingBeforeSpeakingOpensANewSession() {
        val s = connected()
        s.onConnected(false)
        s.onConnected(true)
        assertEquals(Out.Start("/p/proyecto", "haiku", false), sent.last())
        s.onMessage(In.Ready("s2", proyecto))
        assertEquals(Screen.Conversation, s.screen)
        assertEquals("s2", s.session)
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
        assertEquals(Out.Start("/p/proyecto", "haiku", true), sent.last())
    }

    @Test
    fun theMostRecentFolderIsChosenAndAnEmptyListMeansANewChat() {
        val s = Estado({ sent.add(it) }, NullMic(), player, store)
        s.onConnected(true)
        s.onMessage(In.Hello(emptyList()))
        assertNull("lista vacía: «Charla nueva»", s.folder)
        s.onMessage(In.Hello(listOf(proyecto, Folder("/p/notas", "notas"))))
        assertEquals(proyecto, s.folder)
        s.start()
        assertEquals(Out.Start("/p/proyecto", "haiku", false), sent.last())
    }

    @Test
    fun aChosenFolderSurvivesAnotherHello() {
        val notas = Folder("/p/notas", "notas")
        val s = Estado({ sent.add(it) }, NullMic(), player, store)
        s.onConnected(true)
        s.onMessage(In.Hello(listOf(proyecto, notas)))
        s.chooseFolder(notas)
        s.onConnected(false)
        s.onConnected(true)
        s.onMessage(In.Hello(listOf(proyecto, notas)))
        assertEquals(notas, s.folder)
        s.chooseFolder(null)
        s.start()
        assertEquals("""{"type":"start","new_folder":true,"model":"haiku","thinking":false}""", sent.last().toJson())
    }

    @Test
    fun aNewChatKeepsTheCreatedFolderWhenReconnecting() {
        val creada = Folder("/charlas/2026-11-12", "2026-11-12")
        val s = Estado({ sent.add(it) }, NullMic(), player, store)
        s.onConnected(true)
        s.onMessage(In.Hello(emptyList()))
        s.start()
        assertEquals(Out.Start(null, "haiku", false), sent.last())
        s.onMessage(In.Ready("s1", creada))
        assertEquals(creada, s.current)
        s.messages.add(Message(Who.You, "Hola"))
        s.onConnected(false)
        s.onConnected(true)
        assertEquals(Out.Start("/charlas/2026-11-12", "haiku", false, "s1"), sent.last())
    }

    @Test
    fun aMissingFolderGoesBackToStartWithTheMessage() {
        val s = Estado({ sent.add(it) }, NullMic(), player, store)
        s.onConnected(true)
        s.onMessage(In.Hello(listOf(proyecto)))
        s.start()
        s.onMessage(In.FolderMissing("La carpeta no existe"))
        assertEquals(Screen.Start, s.screen)
        assertEquals("La carpeta no existe", s.error)
        assertTrue("se puede volver a empezar", !s.starting)
    }

    @Test
    fun thePermissionCardOpensWhenTheQueueIsEmpty() {
        val s = connected()
        askPermission(s)
        player.empty = false
        s.tick(0.0)
        assertEquals("aún suena el aviso", Phase.Speaking, s.phase)
        player.empty = true
        s.tick(0.0)
        assertEquals(Phase.Permission, s.phase)
        assertEquals("git push origin main", s.permission?.detail)
        s.tap()
        assertEquals("tocar para hablar no hace nada", Phase.Permission, s.phase)
        assertTrue(!s.canTalk())
        assertEquals(1, sent.count { it == Out.PttStart })
    }

    @Test
    fun aReminderDuringPermissionKeepsTheCard() {
        val s = connected()
        askPermission(s)
        s.tick(0.0)
        s.onMessage(In.Sentence("Sigo esperando tu permiso. Míralo en pantalla."))
        s.tick(0.0)
        assertEquals(Phase.Permission, s.phase)
        assertEquals("p1", s.permission?.id)
        assertEquals(
            "Necesito tu permiso. Míralo en pantalla. Sigo esperando tu permiso. Míralo en pantalla.",
            s.lastReply(),
        )
    }

    @Test
    fun answeringSendsTheReplyAndTheTurnContinues() {
        val s = connected()
        askPermission(s)
        s.tick(0.0)
        s.answer(true)
        assertEquals(Out.PermissionReply("p1", true), sent.last())
        assertEquals(Phase.Thinking, s.phase)
        assertNull(s.permission)
        s.answer(false)
        assertEquals("sin tarjeta no se envía nada", 1, sent.count { it is Out.PermissionReply })
        s.onMessage(In.State("speaking"))
        s.onMessage(In.Sentence("Hecho."))
        s.onMessage(In.ReplyEnd)
        s.tick(0.0)
        assertEquals(Phase.Idle, s.phase)
        assertEquals(1, s.messages.count { it.who == Who.Claude })
        assertEquals("Necesito tu permiso. Míralo en pantalla. Hecho.", s.lastReply())
    }

    @Test
    fun rejectingSendsFalse() {
        val s = connected()
        askPermission(s)
        s.tick(0.0)
        s.answer(false)
        assertEquals(Out.PermissionReply("p1", false), sent.last())
        assertEquals(Phase.Thinking, s.phase)
    }

    @Test
    fun theCardIsLostWhenTheConnectionDrops() {
        val s = connected()
        askPermission(s)
        s.tick(0.0)
        s.onConnected(false)
        assertEquals(Phase.Idle, s.phase)
        assertNull(s.permission)
        s.onConnected(true)
        s.tick(0.0)
        assertTrue(s.phase != Phase.Permission)
    }

    @Test
    fun anErrorWithTheCardOpenEndsTheTurn() {
        val s = connected()
        askPermission(s)
        s.tick(0.0)
        s.onMessage(In.Error("Claude falló"))
        s.tick(0.0)
        assertEquals(Phase.Idle, s.phase)
        assertNull(s.permission)
    }

    @Test
    fun theBarShowsTheFolderFromReady() {
        val s = connected()
        assertEquals("proyecto", s.current?.name)
        s.newConversation()
        assertNull(s.current)
    }
}

class ProtocoloTest {
    @Test
    fun parsesEveryServerMessage() {
        assertEquals(
            In.Hello(listOf(Folder("/p/a", "a"), Folder("/p/b", "b"))),
            parse("""{"type":"hello","folders":[{"path":"/p/a","name":"a"},{"path":"/p/b","name":"b"}]}"""),
        )
        assertEquals(In.Hello(emptyList()), parse("""{"type":"hello","folders":[]}"""))
        assertEquals(
            In.Ready("s1", Folder("/p/a", "a")),
            parse("""{"type":"ready","model":"haiku","session":"s1","folder":"/p/a","name":"a"}"""),
        )
        assertEquals(
            In.Permission("p1", "Bash", "git push origin main"),
            parse("""{"type":"permission","id":"p1","tool":"Bash","detail":"git push origin main"}"""),
        )
        assertEquals(In.FolderMissing("x"), parse("""{"type":"error","message":"x","code":"folder_missing"}"""))
        assertEquals(In.State("speaking"), parse("""{"type":"state","state":"speaking"}"""))
        assertEquals(In.Transcript(""), parse("""{"type":"transcript","text":""}"""))
        assertEquals(In.Sentence("Hola."), parse("""{"type":"sentence","text":"Hola."}"""))
        assertEquals(In.ReplyEnd, parse("""{"type":"reply_end"}"""))
        assertEquals(In.Error("x"), parse("""{"type":"error","message":"x"}"""))
        assertEquals(In.SessionGone, parse("""{"type":"error","message":"x","code":"session_gone"}"""))
        assertNull(parse("""{"type":"otro"}"""))
        assertNull(parse("no es json"))
    }

    @Test
    fun messagesFollowTheProtocol() {
        assertEquals(
            """{"type":"start","folder":"/p/a","model":"opus","thinking":true}""",
            Out.Start("/p/a", "opus", true).toJson(),
        )
        assertEquals(
            """{"type":"start","new_folder":true,"model":"haiku","thinking":false}""",
            Out.Start(null, "haiku", false).toJson(),
        )
        assertEquals(
            """{"type":"start","folder":"/p/a","model":"opus","thinking":false,"resume":"s1"}""",
            Out.Start("/p/a", "opus", false, "s1").toJson(),
        )
        assertEquals(
            """{"type":"permission_reply","id":"p1","allow":true}""",
            Out.PermissionReply("p1", true).toJson(),
        )
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
