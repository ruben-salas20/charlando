// La bola de metal líquido: el shader `FS` de diseno/maqueta-android.html pasado a AGSL, con el
// preset gunmetal y los valores de `target()` para cada estado.
package io.github.rubensalas.charlando

import android.graphics.RuntimeShader
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ShaderBrush
import kotlin.math.min
import kotlin.math.sin

private const val AGSL = """
uniform float2 r;
uniform float t, flow, scale, angle, irid, contrast, size;
uniform float3 tint, light;

// La maqueta usa fract(sin(x)*43758.5); en la GPU del teléfono ese sin pierde precisión y la
// bola sale a franjas. Este hash sin seno (Dave Hoskins) da el mismo ruido blanco.
float h(float2 p) {
  float3 p3 = fract(float3(p.xyx) * .1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}
float n(float2 p) {
  float2 i = floor(p), f = fract(p); f = f * f * (3. - 2. * f);
  return mix(mix(h(i), h(i + float2(1, 0)), f.x), mix(h(i + float2(0, 1)), h(i + float2(1, 1)), f.x), f.y);
}
float fbm(float2 p) {
  float v = 0., a = .5;
  for (int k = 0; k < 5; k++) { v += a * n(p); p = p * 2.03 + float2(1.7, 9.2); a *= .5; }
  return v;
}
half4 main(float2 fc) {
  // WebGL cuenta y hacia arriba y Android hacia abajo.
  float2 g = float2(fc.x, r.y - fc.y);
  float2 uv = (g * 2. - r) / r.y / size;
  float d = length(uv);
  if (d > 1.) return half4(0.);
  float2 q = uv * scale;
  float2 w = float2(fbm(q + t * .25), fbm(q + float2(5.2, 1.3) - t * .2));
  float e = fbm(q + flow * 2.2 * w + t * .1);
  float3 nrm = normalize(float3(uv * (.9 + .6 * flow) + (w - .5) * flow * 1.4, sqrt(max(0., 1. - d * d))));
  float3 rf = reflect(float3(0, 0, -1), nrm);
  float c = cos(angle), s = sin(angle);
  rf.xy = float2x2(c, -s, s, c) * rf.xy;
  float sky = smoothstep(-.15, .55, rf.y + (e - .5) * 1.2 * flow);
  float band = exp(-pow((rf.y - .35 + (e - .5) * .8) * 5., 2.));
  float3 col = mix(float3(.035), tint, sky) + band * .9 * tint * light;
  col *= .75 + .25 * nrm.z;
  float bend = 1. - nrm.z;
  col += irid * bend * (.5 + .5 * cos(6.2832 * (e * 2. + float3(0., .33, .67))));
  col = pow(col, float3(contrast));
  float rim = smoothstep(.93, 1., d); col = mix(col, tint * .3, rim);
  float a = smoothstep(1., .985, d);
  return half4(half3(col * a), half(a));
}
"""

private fun hex(v: Int) = floatArrayOf((v shr 16 and 255) / 255f, (v shr 8 and 255) / 255f, (v and 255) / 255f)

private val TINT = hex(0x8d949e)
private val LIGHT = hex(0xe8f0ff)
private const val IRID = .12f
private const val CONTRAST = 1.35f
private val REC_LIGHT = hex(0xff5a5a)

class OrbParams(
    val flow: Float, val speed: Float, val scale: Float, val size: Float,
    val irid: Float, val contrast: Float, val light: FloatArray,
)

/** `target()` de la maqueta, con «Pensando» en modo barrido. `now` en milisegundos. */
fun target(phase: Phase, connected: Boolean, now: Double, lv: Float): OrbParams {
    if (!connected) return OrbParams(0f, 0f, 1.6f, .88f, 0f, CONTRAST * 1.9f, LIGHT)
    return when (phase) {
        Phase.Idle, Phase.Permission -> OrbParams(.08f, .15f, 1.6f, 1f + .025f * sin(now / 650).toFloat(), IRID, CONTRAST, LIGHT)
        Phase.Recording -> OrbParams(.25f + .75f * lv, -.9f, 2.4f, .97f - .05f * lv, IRID, CONTRAST, REC_LIGHT)
        Phase.Thinking -> OrbParams(.15f, .2f, 1.6f, .94f, IRID, CONTRAST, LIGHT)
        Phase.Speaking -> OrbParams(.2f + .8f * lv, 1f, 1.4f, .96f + .07f * lv, IRID, CONTRAST, LIGHT)
    }
}

/** El `frame()` de la maqueta: se acerca a `target()` y acumula el tiempo del shader. */
class OrbMotion {
    var flow = .08f; var speed = .15f; var scale = 1.6f; var size = 1f
    var irid = IRID; var contrast = CONTRAST
    val light = LIGHT.copyOf()
    var angle = 0f; var t = 0f

    fun step(dt: Float, now: Double, phase: Phase, connected: Boolean, lv: Float) {
        val g = target(phase, connected, now, lv)
        val k = min(1f, dt * 7f)
        flow += (g.flow - flow) * k
        speed += (g.speed - speed) * k
        scale += (g.scale - scale) * k
        size += (g.size - size) * k
        irid += (g.irid - irid) * k
        contrast += (g.contrast - contrast) * k
        for (i in 0..2) light[i] += (g.light[i] - light[i]) * k
        angle = if (phase == Phase.Thinking && connected) 1.1f * sin(now / 700).toFloat() else angle + (0f - angle) * k
        t += dt * speed
    }
}

@Composable
fun Bola(phase: Phase, connected: Boolean, level: Float, modifier: Modifier = Modifier) {
    val shader = remember { RuntimeShader(AGSL) }
    val brush = remember { ShaderBrush(shader) }
    val motion = remember { OrbMotion() }
    var frame by remember { mutableLongStateOf(0L) }
    val st by rememberUpdatedState(Triple(phase, connected, level))
    LaunchedEffect(Unit) {
        var last = 0L
        while (true) {
            withFrameNanos { ns ->
                val dt = if (last == 0L) 0f else min(.05f, (ns - last) / 1e9f)
                last = ns
                motion.step(dt, ns / 1e6, st.first, st.second, st.third)
                frame = ns
            }
        }
    }
    Canvas(modifier) {
        // Leer `frame` hace que se redibuje en cada fotograma.
        if (frame < 0) return@Canvas
        shader.setFloatUniform("r", size.width, size.height)
        shader.setFloatUniform("t", motion.t)
        shader.setFloatUniform("flow", motion.flow)
        shader.setFloatUniform("scale", motion.scale)
        shader.setFloatUniform("size", motion.size)
        shader.setFloatUniform("angle", motion.angle)
        shader.setFloatUniform("irid", motion.irid)
        shader.setFloatUniform("contrast", motion.contrast)
        shader.setFloatUniform("tint", TINT)
        shader.setFloatUniform("light", motion.light)
        drawRect(brush)
    }
}
