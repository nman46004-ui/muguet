package com.muguet

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.net.Uri
import android.os.Bundle
import android.text.format.Formatter
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import java.io.File
import java.io.IOException

class LauncherActivity : AppCompatActivity() {

    private val zipPicker = registerForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri: Uri? ->
        if (uri != null) {
            viewModel.importGameZip(uri)
        }
    }

    private lateinit var viewModel: LauncherViewModel

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        viewModel = LauncherViewModel(applicationContext)

        setContent {
            MuguetTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    LauncherScreen(viewModel, zipPicker)
                }
            }
        }
    }
}

@Composable
fun LauncherScreen(viewModel: LauncherViewModel, zipPicker: androidx.activity.result.ActivityResultLauncher<Array<String>>) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp)
            .verticalScroll(rememberScrollState()),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                text = "MUGUET",
                fontSize = 40.sp,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.primary
            )
            Text(
                text = "GTA V on Android",
                fontSize = 14.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }

        if (!viewModel.gameReady || viewModel.importing) {
            ImportSection(viewModel, zipPicker)
        } else {
            SettingsSection(viewModel)
        }

        if (viewModel.gameReady) {
            Button(
                onClick = { viewModel.play() },
                modifier = Modifier.fillMaxWidth().height(56.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.primary,
                    contentColor = MaterialTheme.colorScheme.onPrimary
                )
            ) {
                Text(text = "PLAY", fontSize = 20.sp, fontWeight = FontWeight.Bold)
            }

            Text(
                text = "First start compiles shaders (~30-60s)",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth()
            )
        }

        Text(
            text = "proudly made by c22dev",
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
        )
    }
}

@Composable
fun ImportSection(viewModel: LauncherViewModel, zipPicker: androidx.activity.result.ActivityResultLauncher<Array<String>>) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        elevation = CardDefaults.cardElevation(defaultElevation = 8.dp)
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            if (viewModel.importing) {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    LinearProgressIndicator(
                        progress = { viewModel.importProgress },
                        modifier = Modifier.fillMaxWidth()
                    )
                    Text(
                        text = "Importing… ${(viewModel.importProgress * 100).toInt()}%",
                        fontSize = 14.sp
                    )
                }
            } else {
                Text(
                    text = "Game data not found.\nImport your GTA V WebAssembly .zip",
                    fontSize = 16.sp,
                    textAlign = TextAlign.Center,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Button(
                    onClick = { zipPicker.launch(arrayOf("application/zip")) },
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer
                    )
                ) {
                    Text(text = "Choose game data .zip", fontSize = 16.sp)
                }
            }
            viewModel.importError?.let { error ->
                Text(
                    text = error,
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.error,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth()
                )
            }
        }
    }
}

@Composable
fun SettingsSection(viewModel: LauncherViewModel) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        elevation = CardDefaults.cardElevation(defaultElevation = 4.dp)
    ) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            // Mode
            DropdownSetting(
                label = "Mode",
                selectedText = if (viewModel.mode == 1) "Story Mode" else "Sandbox",
                items = listOf("Story Mode" to 1, "Sandbox" to 2),
                onSelect = { viewModel.mode = it }
            )
            // Resolution
            DropdownSetting(
                label = "Resolution",
                selectedText = when (viewModel.scale) {
                    0.5f -> "50% (fastest)"
                    0.75f -> "75%"
                    1.0f -> "100% (sharpest)"
                    else -> "${(viewModel.scale * 100).toInt()}%"
                },
                items = listOf("50% (fastest)" to 0.5f, "75%" to 0.75f, "100% (sharpest)" to 1.0f),
                onSelect = { viewModel.scale = it }
            )
            // Frame limit
            DropdownSetting(
                label = "Frame Limit",
                selectedText = when (viewModel.fpsLimit) {
                    30 -> "30 fps (cooler)"
                    60 -> "60 fps"
                    0 -> "Uncapped"
                    else -> "${viewModel.fpsLimit} fps"
                },
                items = listOf("30 fps (cooler)" to 30, "60 fps" to 60, "Uncapped" to 0),
                onSelect = { viewModel.fpsLimit = it }
            )
            // Touch sensitivity
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(text = "Touch Sensitivity", fontSize = 16.sp, modifier = Modifier.weight(1f))
                Slider(
                    value = viewModel.touchSens,
                    onValueChange = { viewModel.touchSens = it },
                    valueRange = 0.5f..6f,
                    steps = 10,
                    modifier = Modifier.weight(1.5f)
                )
                Text(
                    text = String.format("%.1fx", viewModel.touchSens),
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.width(44.dp),
                    textAlign = TextAlign.End
                )
            }
            // Fullscreen
            SwitchSetting(label = "Fullscreen", checked = viewModel.fullscreen, onCheckedChange = { viewModel.fullscreen = it })
            // Mute
            SwitchSetting(label = "Mute Audio", checked = viewModel.mute, onCheckedChange = { viewModel.mute = it })

            // Multiplayer
            HorizontalDivider()
            Text(
                text = "Multiplayer",
                fontSize = 18.sp,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.primary
            )
            SwitchSetting(
                label = "Play with friends",
                checked = viewModel.multiplayer,
                onCheckedChange = { viewModel.multiplayer = it }
            )
            if (viewModel.multiplayer) {
                OutlinedTextField(
                    value = viewModel.playerName,
                    onValueChange = { viewModel.playerName = it },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = { Text("Your name") }
                )
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    FilterChip(
                        selected = viewModel.host,
                        onClick = { viewModel.host = true },
                        label = { Text("Host") },
                        modifier = Modifier.weight(1f)
                    )
                    FilterChip(
                        selected = !viewModel.host,
                        onClick = { viewModel.host = false },
                        label = { Text("Join") },
                        modifier = Modifier.weight(1f)
                    )
                }
                if (viewModel.host) {
                    Text(
                        text = "Friends join with: ${viewModel.localIPs.joinToString(" or ")}",
                        fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Text(
                        text = "Forward UDP port 47520 for internet play",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.fillMaxWidth()
                    )
                } else {
                    OutlinedTextField(
                        value = viewModel.serverAddress,
                        onValueChange = { viewModel.serverAddress = it },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        label = { Text("Host address") },
                        placeholder = { Text("192.168.1.20") }
                    )
                }
            }
        }
    }
}

@Composable
fun <T> DropdownSetting(
    label: String,
    selectedText: String,
    items: List<Pair<String, T>>,
    onSelect: (T) -> Unit
) {
    var expanded by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(text = label, fontSize = 16.sp, modifier = Modifier.weight(1f))
        Box {
            OutlinedButton(
                onClick = { expanded = true },
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.outlinedButtonColors(
                    containerColor = MaterialTheme.colorScheme.surfaceContainer
                )
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = selectedText,
                        fontSize = 14.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                    Icon(
                        imageVector = Icons.Default.ArrowDropDown,
                        contentDescription = null,
                        modifier = Modifier.size(20.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
            DropdownMenu(
                expanded = expanded,
                onDismissRequest = { expanded = false }
            ) {
                items.forEach { (text, value) ->
                    DropdownMenuItem(
                        text = { Text(text) },
                        onClick = {
                            onSelect(value)
                            expanded = false
                        }
                    )
                }
            }
        }
    }
}

@Composable
fun SwitchSetting(label: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(text = label, fontSize = 16.sp)
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

@Composable
fun MuguetTheme(
    darkTheme: Boolean = true,
    content: @Composable () -> Unit
) {
    val colorScheme = if (darkTheme) {
        darkColorScheme(
            primary = Color(0xFFBB86FC),
            primaryContainer = Color(0xFF3700B3),
            secondary = Color(0xFF03DAC5),
            background = Color(0xFF121212),
            surface = Color(0xFF1E1E1E),
            surfaceContainer = Color(0xFF2A2A2A),
            error = Color(0xFFCF6679)
        )
    } else {
        lightColorScheme(
            primary = Color(0xFF6200EE),
            secondary = Color(0xFF018786),
            background = Color(0xFFFFFFFF),
            surface = Color(0xFFF5F5F5)
        )
    }
    MaterialTheme(
        colorScheme = colorScheme,
        typography = Typography(),
        content = content
    )
}

class LauncherViewModel(private val context: Context) {
    private val prefs: SharedPreferences = context.getSharedPreferences("muguet_config", Context.MODE_PRIVATE)

    var gameReady by mutableStateOf(false)
        private set

    var importing by mutableStateOf(false)
        private set

    var importProgress by mutableFloatStateOf(0f)
        private set

    var importError by mutableStateOf<String?>(null)
        private set

    // Settings — backed by SharedPreferences, exposed as Compose state
    var mode by mutableIntStateOf(prefs.getInt("start_mode", 2))
        private set

    var scale by mutableFloatStateOf(prefs.getFloat("scale", 0.5f))
        private set

    var fpsLimit by mutableIntStateOf(prefs.getInt("fps_limit", 60))
        private set

    var touchSens by mutableFloatStateOf(prefs.getFloat("touch_sens", 2.0f))
        private set

    var fullscreen by mutableStateOf(prefs.getBoolean("fullscreen", true))
        private set

    var mute by mutableStateOf(prefs.getBoolean("mute", false))
        private set

    var multiplayer by mutableStateOf(prefs.getBoolean("multiplayer", false))
        private set

    var host by mutableStateOf(prefs.getBoolean("host", true))
        private set

    var serverAddress by mutableStateOf(prefs.getString("server", "") ?: "")
        private set

    var playerName by mutableStateOf(prefs.getString("player_name", "") ?: "")
        private set

    val localIPs: List<String>
        get() {
            return try {
                val wifiManager = context.getSystemService(Context.WIFI_SERVICE) as android.net.wifi.WifiManager
                val ip = wifiManager.connectionInfo.ipAddress
                if (ip != 0) listOf(Formatter.formatIpAddress(ip)) else listOf("?.?.?.?")
            } catch (e: Exception) {
                listOf("?.?.?.?")
            }
        }

    init {
        checkGameReady()
    }

    private fun checkGameReady() {
        val gameDir = File(context.getExternalFilesDir(null), "muguet/game")
        gameReady = gameDir.exists() &&
            File(gameDir, "data/manifest.json").exists() &&
            File(gameDir, "b").listFiles()?.any { it.isDirectory && File(it, "shaders/index.json").exists() } == true
    }

    fun importGameZip(uri: Uri) {
        importing = true
        importError = null
        importProgress = 0f

        CoroutineScope(Dispatchers.IO).launch {
            try {
                val gameDir = File(context.getExternalFilesDir(null), "muguet/game")
                gameDir.mkdirs()

                val inputStream = context.contentResolver.openInputStream(uri)
                    ?: throw IOException("Cannot open zip")

                // Extract zip directly in Kotlin (no external process needed)
                val buffer = ByteArray(8192)
                java.util.zip.ZipInputStream(inputStream).use { zis ->
                    var entry = zis.nextEntry
                    var total = 0L
                    var done = 0L

                    // First pass: count total
                    val entries = mutableListOf<java.util.zip.ZipEntry>()
                    while (entry != null) {
                        entries.add(entry)
                        total += entry.size
                        entry = zis.nextEntry
                    }

                    // Reset and extract
                    inputStream.close()
                    val stream2 = context.contentResolver.openInputStream(uri)
                        ?: throw IOException("Cannot reopen zip")
                    java.util.zip.ZipInputStream(stream2).use { zis2 ->
                        var e2 = zis2.nextEntry
                        while (e2 != null) {
                            val outFile = File(gameDir, e2.name)
                            if (e2.isDirectory) {
                                outFile.mkdirs()
                            } else {
                                outFile.parentFile?.mkdirs()
                                outFile.outputStream().use { out ->
                                    var n: Int
                                    while (zis2.read(buffer).also { n = it } > 0) {
                                        out.write(buffer, 0, n)
                                        done += n
                                        if (total > 0) {
                                            importProgress = (done.toFloat() / total).coerceIn(0f, 1f)
                                        }
                                    }
                                }
                            }
                            zis2.closeEntry()
                            e2 = zis2.nextEntry
                        }
                    }
                }

                importing = false
                checkGameReady()
            } catch (e: Exception) {
                importing = false
                importError = "Import failed: ${e.message}"
            }
        }
    }

    fun play() {
        // Persist all settings
        prefs.edit().apply {
            putInt("start_mode", mode)
            putFloat("scale", scale)
            putInt("fps_limit", fpsLimit)
            putFloat("touch_sens", touchSens)
            putBoolean("fullscreen", fullscreen)
            putBoolean("mute", mute)
            putBoolean("multiplayer", multiplayer)
            putBoolean("host", host)
            putString("server", serverAddress.trim())
            putString("player_name", playerName.trim())
        }.apply()

        // Launch game activity with config
        val intent = Intent(context, MainActivity::class.java).apply {
            putExtra("start_mode", mode)
            putExtra("scale", scale)
            putExtra("fps_limit", fpsLimit)
            putExtra("touch_sens", touchSens)
            putExtra("fullscreen", fullscreen)
            putExtra("mute", mute)
            putExtra("multiplayer", multiplayer)
            putExtra("host", host)
            putExtra("server", serverAddress.trim())
            putExtra("player_name", playerName.trim())
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK
        }
        context.startActivity(intent)
    }
}