package uk.ccez.keys

import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.view.inputmethod.InputMethodManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Turn the keyboard on, choose it, try it. */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // Dictation needs the microphone; a keyboard can't ask, so the app does, once.
        if (checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(android.Manifest.permission.RECORD_AUDIO), 1)
        }
        setContent {
            var text by remember { mutableStateOf("") }
            val buttons = ButtonDefaults.buttonColors(containerColor = Colors.key, contentColor = Colors.fg)
            Column(
                Modifier.fillMaxSize().background(Colors.bg).safeDrawingPadding().imePadding().padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Text("ccez-keys", color = Colors.fg, fontSize = 22.sp)
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Button(onClick = { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) }, colors = buttons) { Text("Turn on") }
                    Button(onClick = { getSystemService(InputMethodManager::class.java)?.showInputMethodPicker() }, colors = buttons) { Text("Choose") }
                }
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    modifier = Modifier.fillMaxWidth().testTag("try"),
                    minLines = 3,
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedTextColor = Colors.fg,
                        unfocusedTextColor = Colors.fg,
                        focusedBorderColor = Colors.muted,
                        unfocusedBorderColor = Colors.line,
                        cursorColor = Colors.accent,
                    ),
                )
                Text(BuildConfig.VERSION_NAME, color = Colors.dim, fontSize = 12.sp)
            }
        }
    }
}
