package io.github.rubensalas.charlando

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme

class MainActivity : ComponentActivity() {
    private val charla: Charla by viewModels()
    private val askMic = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) intent.getStringExtra("escena")?.let { escena(charla, it) }
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                App(charla, onNeedMic = ::micAllowed)
            }
        }
    }

    /** True si ya hay permiso; si no, lo pide y el usuario vuelve a tocar. */
    private fun micAllowed(): Boolean {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) return true
        askMic.launch(Manifest.permission.RECORD_AUDIO)
        return false
    }
}
