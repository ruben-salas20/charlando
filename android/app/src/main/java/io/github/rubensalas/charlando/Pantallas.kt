// Las pantallas, con los colores, tamaños y textos de diseno/maqueta-android.html.
package io.github.rubensalas.charlando

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.math.sin

private val Bg = Color(0xFF141414)
private val L1 = Color(0xFF1C1C1C)
private val L2 = Color(0xFF232323)
private val L3 = Color(0xFF2C2C2C)
private val Line = Color.White.copy(alpha = .08f)
private val Fg = Color(0xFFEDEDED)
private val Fg2 = Color(0xFFA3A3A3)
private val Fg3 = Color(0xFF6E6E6E)
private val Accent = Color(0xFF6F8FFF)
private val Rec = Color(0xFFE5484D)
private val Ok = Color(0xFF46A758)

private val Inter = FontFamily(
    Font(R.font.inter_regular, FontWeight.Normal),
    Font(R.font.inter_medium, FontWeight.Medium),
    Font(R.font.inter_semibold, FontWeight.SemiBold),
)
private val Mono = FontFamily(Font(R.font.plex_mono, FontWeight.Normal))

private fun style(size: TextUnit, weight: FontWeight = FontWeight.Normal, color: Color = Fg) =
    TextStyle(fontFamily = Inter, fontSize = size, fontWeight = weight, color = color, lineHeight = 1.45.em)

private val Body = 15.sp

private fun Modifier.tap(enabled: Boolean = true, role: Role = Role.Button, onClick: () -> Unit) =
    clickable(enabled = enabled, role = role, interactionSource = null, indication = null, onClick = onClick)

private fun Modifier.box(fill: Color, shape: Shape, line: Color = Line, width: Dp = 1.dp) =
    clip(shape).background(fill).border(width, line, shape)

@Composable
fun App(charla: Charla, onNeedMic: () -> Boolean) {
    val s = charla.estado
    Box(Modifier.fillMaxSize().background(Bg)) {
        Column(Modifier.fillMaxSize().systemBarsPadding()) {
            Bar(s, onSettings = { charla.settingsOpen = true })
            when (s.screen) {
                Screen.Start -> Inicio(s, onStart = { onNeedMic(); s.start() })
                Screen.Conversation -> Conversacion(s, onTap = { if (onNeedMic()) s.tap() })
            }
        }
        Ajustes(charla.settingsOpen, charla.url, onSave = charla::saveUrl, onDismiss = { charla.settingsOpen = false })
    }
}

@Composable
private fun Bar(s: Estado, onSettings: () -> Unit) {
    Box(
        Modifier.fillMaxWidth().height(52.dp).drawLine(bottom = true)
            .padding(start = 18.dp, end = 12.dp),
    ) {
        Row(Modifier.align(Alignment.CenterStart), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(8.dp).clip(CircleShape).background(if (s.connected) Ok else Rec))
            Spacer(Modifier.size(8.dp))
            Text("Charlando", style = style(16.sp, FontWeight.SemiBold))
        }
        if (s.screen == Screen.Conversation) {
            Segmento(s, Modifier.align(Alignment.Center))
            IconButton(R.drawable.ic_add, "Nueva conversación", Modifier.align(Alignment.CenterEnd)) { s.newConversation() }
        } else {
            IconButton(R.drawable.ic_settings, "Ajustes", Modifier.align(Alignment.CenterEnd), onSettings)
        }
    }
}

@Composable
private fun IconButton(icon: Int, label: String, modifier: Modifier, onClick: () -> Unit) {
    Box(
        modifier.size(40.dp).clip(CircleShape).clickable(onClick = onClick, role = Role.Button, onClickLabel = label),
        contentAlignment = Alignment.Center,
    ) {
        Icon(painterResource(icon), contentDescription = label, tint = Fg2, modifier = Modifier.size(22.dp))
    }
}

/** El borde de 1 px de la maqueta, arriba o abajo. */
private fun Modifier.drawLine(bottom: Boolean) = drawBehind {
    val y = if (bottom) size.height - .5.dp.toPx() else .5.dp.toPx()
    drawLine(Line, Offset(0f, y), Offset(size.width, y), 1.dp.toPx())
}

@Composable
private fun Segmento(s: Estado, modifier: Modifier) {
    Row(modifier.clip(RoundedCornerShape(7.dp)).background(L2).padding(2.dp)) {
        for ((v, label) in listOf(View.Chat to "Chat", View.Voice to "Voz")) {
            val on = s.view == v
            Text(
                label,
                style = style(13.sp, color = if (on) Fg else Fg2),
                modifier = Modifier.clip(RoundedCornerShape(5.dp)).background(if (on) L3 else Color.Transparent)
                    .tap(role = Role.Tab) { s.changeView(v) }.padding(horizontal = 12.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun Inicio(s: Estado, onStart: () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 22.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp, Alignment.CenterVertically),
    ) {
        Column {
            Text("Charlando", style = style(26.sp, FontWeight.SemiBold))
            Text("Elige con quién hablar.", style = style(Body, color = Fg2), modifier = Modifier.padding(top = 4.dp))
        }
        Column {
            Text("Modelo", style = style(12.sp, color = Fg3), modifier = Modifier.padding(bottom = 8.dp))
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (m in Model.entries) {
                    val on = s.model == m
                    Row(
                        Modifier.fillMaxWidth().box(if (on) L2 else L1, RoundedCornerShape(12.dp), if (on) Accent else Line)
                            .tap(role = Role.RadioButton) { s.model = m }.padding(horizontal = 14.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Box(Modifier.size(16.dp).border(if (on) 5.dp else 1.5.dp, if (on) Accent else Fg3, CircleShape))
                        Column {
                            Text(m.title, style = style(Body, FontWeight.Medium))
                            Text(m.blurb, style = style(13.sp, color = Fg3))
                        }
                    }
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().tap(role = Role.Switch) { s.toggleThinking() },
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text("Razonar", style = style(Body))
                Text("Claude piensa cuando lo necesita. Más lento.", style = style(13.sp, color = Fg3))
            }
            Box(Modifier.size(44.dp, 26.dp).clip(RoundedCornerShape(13.dp)).background(if (s.thinking) Accent else L3)) {
                Box(
                    Modifier.offset(x = if (s.thinking) 21.dp else 3.dp, y = 3.dp).size(20.dp)
                        .clip(CircleShape).background(Fg),
                )
            }
        }
        val ready = s.connected && !s.starting
        BotonClaro("Empezar", enabled = ready, onClick = onStart)
    }
}

@Composable
private fun BotonClaro(label: String, enabled: Boolean = true, onClick: () -> Unit) {
    Box(
        Modifier.fillMaxWidth().alpha(if (enabled) 1f else .5f).clip(RoundedCornerShape(14.dp))
            .background(Fg).tap(enabled = enabled, onClick = onClick).padding(14.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = style(Body, FontWeight.SemiBold, Bg))
    }
}

/** Hoja de ajustes dibujada en la propia ventana: ModalBottomSheet abre otra y pinta clara la barra de navegación. */
@Composable
private fun Ajustes(open: Boolean, url: String, onSave: (String) -> Unit, onDismiss: () -> Unit) {
    BackHandler(enabled = open, onBack = onDismiss)
    AnimatedVisibility(open, enter = fadeIn(), exit = fadeOut()) {
        Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = .5f)).tap(role = Role.Button, onClick = onDismiss))
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomCenter) {
        AnimatedVisibility(open, enter = slideInVertically { it }, exit = slideOutVertically { it }) {
            HojaAjustes(url, onSave)
        }
    }
}

@Composable
private fun HojaAjustes(url: String, onSave: (String) -> Unit) {
    var text by remember { mutableStateOf(url) }
    val shape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp)
    Column(
        Modifier.fillMaxWidth().clip(shape).background(L1).border(1.dp, Line, shape)
            .tap(role = Role.Button) {}
            .navigationBarsPadding().imePadding()
            .padding(start = 22.dp, end = 22.dp, top = 14.dp, bottom = 28.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Box(Modifier.padding(bottom = 6.dp).size(36.dp, 4.dp).clip(RoundedCornerShape(2.dp)).background(L3).align(Alignment.CenterHorizontally))
        Text("Ajustes", style = style(17.sp, FontWeight.SemiBold))
        Column {
            Text("Servidor", style = style(12.sp, color = Fg3), modifier = Modifier.padding(bottom = 8.dp))
            val mono = TextStyle(fontFamily = Mono, fontSize = 13.sp, color = Fg)
            BasicTextField(
                value = text,
                onValueChange = { text = it },
                singleLine = true,
                textStyle = mono,
                cursorBrush = SolidColor(Fg2),
                keyboardOptions = KeyboardOptions(
                    keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done, autoCorrectEnabled = false,
                ),
                modifier = Modifier.fillMaxWidth(),
                decorationBox = { inner ->
                    Box(Modifier.fillMaxWidth().box(Bg, RoundedCornerShape(10.dp)).padding(12.dp)) {
                        if (text.isEmpty()) Text("ws://IP:8765/ws", style = mono.copy(color = Fg3))
                        inner()
                    }
                },
            )
            Text(
                "La dirección de tu servidor de Charlando. Con Tailscale, la IP del equipo donde corre.",
                style = style(12.5.sp, color = Fg3),
                modifier = Modifier.padding(top = 6.dp),
            )
        }
        BotonClaro("Guardar") { onSave(text) }
    }
}

private fun phaseLabel(s: Estado) = if (!s.connected) "Sin conexión. Reintentando…" else when (s.phase) {
    Phase.Idle -> ""
    Phase.Recording -> "Grabando… toca para enviar"
    Phase.Thinking -> "Pensando…"
    Phase.Speaking -> "Claude habla…"
}

private fun bigLabel(s: Estado) = if (!s.connected) "Sin conexión" else when (s.phase) {
    Phase.Idle -> "Toca la bola para hablar"
    Phase.Recording -> "Te escucho"
    Phase.Thinking -> "Pensando…"
    Phase.Speaking -> "Hablando"
}

@Composable
private fun Conversacion(s: Estado, onTap: () -> Unit) {
    val view = LocalView.current
    DisposableEffect(Unit) {
        view.keepScreenOn = true
        onDispose { view.keepScreenOn = false }
    }
    Column(Modifier.fillMaxSize()) {
        when (s.view) {
            View.Chat -> {
                Chat(s, Modifier.weight(1f))
                Pie(s, onTap)
            }
            View.Voice -> {
                Voz(s, onTap, Modifier.weight(1f))
                Spacer(Modifier.height(22.dp))
            }
        }
    }
}

private const val CARET = "caret"

private fun caretContent(size: TextUnit) = mapOf(
    CARET to InlineTextContent(Placeholder(10.sp, size, PlaceholderVerticalAlign.TextCenter)) {
        Box(Modifier.padding(start = 2.dp).size(8.dp, 15.dp).clip(RoundedCornerShape(2.dp)).background(Fg2))
    },
)

/** El texto de una respuesta, con el cursor al final mientras Claude habla. */
@Composable
private fun Respuesta(text: String, streaming: Boolean, style: TextStyle, modifier: Modifier = Modifier) {
    if (!streaming) {
        Text(text, style = style, modifier = modifier)
        return
    }
    val content = buildAnnotatedString {
        append(text)
        appendInlineContent(CARET, "▌")
    }
    Text(content, style = style, inlineContent = caretContent(15.sp), modifier = modifier)
}

@Composable
private fun Chat(s: Estado, modifier: Modifier) {
    val scroll = rememberScrollState()
    val speaking = s.phase == Phase.Speaking
    val n = s.messages.size
    Column(
        modifier.fillMaxWidth().verticalScroll(scroll).padding(18.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        s.messages.forEachIndexed { i, m ->
            val streaming = speaking && i == n - 1 && m.who == Who.Claude
            if (m.text.isEmpty() && !streaming) return@forEachIndexed
            Column {
                Text(if (m.who == Who.You) "Tú" else "Claude", style = style(12.sp, color = Fg3),
                    modifier = Modifier.padding(bottom = 3.dp))
                Respuesta(m.text, streaming, style(Body, color = if (m.who == Who.You) Fg2 else Fg))
            }
        }
        s.error?.let { Text("Error: $it", style = style(13.sp, color = Rec)) }
    }
    // La respuesta crece mientras suena: se sigue el final.
    LaunchedEffect(scroll.maxValue) { scroll.scrollTo(scroll.maxValue) }
}

@Composable
private fun Medidor(s: Estado, color: Color) {
    val on = s.connected && (s.phase == Phase.Recording || s.phase == Phase.Speaking)
    Row(Modifier.height(20.dp), horizontalArrangement = Arrangement.spacedBy(2.dp), verticalAlignment = Alignment.CenterVertically) {
        for (i in 0 until 10) {
            val shape = abs(sin(i * 1.7f))
            val h = if (on) 4f + (shape * 14f * s.level).roundToInt() else 4f
            Box(Modifier.size(3.dp, h.dp).alpha(.7f).clip(RoundedCornerShape(2.dp)).background(color))
        }
    }
}

@Composable
private fun Pie(s: Estado, onTap: () -> Unit) {
    val rec = s.phase == Phase.Recording
    val busy = !s.connected || s.phase == Phase.Thinking || s.phase == Phase.Speaking
    Column(
        Modifier.fillMaxWidth().drawLine(bottom = false).padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 22.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        val label = phaseLabel(s)
        if (label.isNotEmpty()) Text(label, style = style(13.sp, color = Fg2))
        val shape = RoundedCornerShape(30.dp)
        val fg = if (rec) Color.White else Fg
        Row(
            Modifier.fillMaxWidth().height(60.dp).alpha(if (busy) .45f else 1f)
                .box(if (rec) Rec else L2, shape, if (rec) Color.Transparent else Line)
                .tap(enabled = s.canTalk() || rec, onClick = onTap),
            horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Medidor(s, fg)
            Text(if (rec) "Toca para enviar" else "Toca para hablar", style = style(Body, FontWeight.SemiBold, fg))
        }
    }
}

/** Renglones visibles de la pregunta (luego «…») y de la respuesta (los últimos). */
private const val QUESTION_LINES = 3
private const val REPLY_LINES = 4
/** Lo que ocupa el desvanecido de arriba de la respuesta. */
private val ReplyFade = 22.dp
private val VoiceGap = 8.dp

/** Desvanece lo que dibuja el elemento en sus `top` primeros puntos. */
private fun Modifier.fadeTop(top: Dp) = graphicsLayer(compositingStrategy = CompositingStrategy.Offscreen)
    .drawWithContent {
        drawContent()
        val stop = (top.toPx() / size.height).coerceIn(0f, 1f)
        drawRect(Brush.verticalGradient(0f to Color.Transparent, stop to Color.Black), blendMode = BlendMode.DstIn)
    }

@Composable
private fun Voz(s: Estado, onTap: () -> Unit, modifier: Modifier) {
    val showReply = s.phase == Phase.Speaking || s.phase == Phase.Idle
    val line = with(LocalDensity.current) { (14.sp * 1.45f).toDp() }
    val replyMax = line * REPLY_LINES + ReplyFade
    Column(
        modifier.fillMaxWidth().padding(20.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(18.dp, Alignment.CenterVertically),
    ) {
        Bola(
            s.phase, s.connected, s.level,
            Modifier.size(220.dp).clip(CircleShape)
                .tap(enabled = s.canTalk() || s.phase == Phase.Recording, onClick = onTap),
        )
        Text(bigLabel(s), style = style(19.sp, FontWeight.SemiBold), textAlign = TextAlign.Center)
        // Alto fijo para que la bola no se mueva con el texto.
        Column(
            Modifier.widthIn(max = 300.dp).height(line * QUESTION_LINES + VoiceGap + replyMax),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            s.lastQuestion()?.let {
                Text("«$it»", style = style(14.sp, color = Fg3), textAlign = TextAlign.Center,
                    maxLines = QUESTION_LINES, overflow = TextOverflow.Ellipsis)
            }
            // La respuesta sigue su final y se desvanece arriba. El margen superior del contenido
            // cae bajo el desvanecido, así que una respuesta corta se ve entera.
            val scroll = rememberScrollState()
            Column(
                Modifier.padding(top = VoiceGap).heightIn(max = replyMax).fadeTop(ReplyFade)
                    .verticalScroll(scroll).padding(top = ReplyFade),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                val reply = s.lastReply()
                if (showReply && reply != null) {
                    Respuesta(reply, s.phase == Phase.Speaking, style(14.sp, color = Fg2).copy(textAlign = TextAlign.Center))
                }
                s.error?.let { Text("Error: $it", style = style(13.sp, color = Rec), textAlign = TextAlign.Center) }
            }
            LaunchedEffect(scroll.maxValue) { scroll.scrollTo(scroll.maxValue) }
        }
    }
}
