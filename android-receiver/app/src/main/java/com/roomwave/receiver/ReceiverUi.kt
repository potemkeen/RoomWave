package com.roomwave.receiver

import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Green = Color(0xFF28614F)
private val Ink = Color(0xFF203532)
private val Muted = Color(0xFF66796C)
private val Page = Color(0xFFF5F6F3)
private val Soft = Color(0xFFE9EFE7)

@Composable
private fun RoomWaveTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = lightColorScheme(primary = Green, onPrimary = Color.White,
        background = Page, onBackground = Ink, surface = Color.White, onSurface = Ink,
        surfaceVariant = Soft, onSurfaceVariant = Muted, outline = Color(0xFFD8E0DA)), content = content)
}

@Composable
fun RoomWaveApp(onStart: () -> Unit) {
    var diagnostics by rememberSaveable { mutableStateOf(false) }
    RoomWaveTheme {
        Surface(Modifier.fillMaxSize(), color = Page) {
            if (diagnostics) {
                BackHandler { diagnostics = false }
                DiagnosticsScreen(onBack = { diagnostics = false })
            } else {
                val service = AudioReceiverService.current
                val advertiser = service?.advertiser
                ReceiverScreen(
                    name = advertiser?.deviceName ?: "${Build.MANUFACTURER} ${Build.MODEL}",
                    running = service != null,
                    ready = advertiser?.registered == true,
                    connected = service?.connected == true,
                    hasError = service?.streamError != null || AudioReceiverService.lastError != null || advertiser?.error != null,
                    onStart = onStart,
                    onDisconnect = { service?.disconnect() },
                    onStop = { service?.stopSelf() },
                    onDiagnostics = { diagnostics = true }
                )
            }
        }
    }
}

@Composable
private fun WaveMark(modifier: Modifier = Modifier, color: Color = Green) {
    Canvas(modifier) {
        val heights = listOf(.25f, .6f, 1f, .6f, .25f)
        heights.forEachIndexed { index, height ->
            val x = size.width * (index + 1) / 6
            drawLine(color, Offset(x, size.height * (1 - height) / 2),
                Offset(x, size.height * (1 + height) / 2), strokeWidth = size.width / 17, cap = StrokeCap.Round)
        }
    }
}

@Composable
private fun ReceiverScreen(
    name: String, running: Boolean, ready: Boolean, connected: Boolean, hasError: Boolean,
    onStart: () -> Unit, onDisconnect: () -> Unit, onStop: () -> Unit, onDiagnostics: () -> Unit
) {
    var menu by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().safeDrawingPadding().verticalScroll(rememberScrollState())
        .padding(horizontal = 24.dp, vertical = 20.dp), verticalArrangement = Arrangement.spacedBy(24.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(38.dp).background(Green, RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
                WaveMark(Modifier.size(23.dp), Color.White)
            }
            Text("RoomWave", Modifier.padding(start = 10.dp).weight(1f), fontSize = 21.sp, fontWeight = FontWeight.SemiBold)
            Box {
                TextButton(onClick = { menu = true }) { Text("Меню") }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text("Диагностика") }, onClick = { menu = false; onDiagnostics() })
                    if (running) DropdownMenuItem(text = { Text("Остановить приёмник") }, onClick = { menu = false; onStop() })
                }
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("ВАША ЗВУКОВАЯ СИСТЕМА", color = Muted, fontSize = 10.sp, letterSpacing = 1.8.sp, fontWeight = FontWeight.Bold)
            Text(if (connected) "Звук уже здесь." else "Телефон стал\nчастью комнаты.",
                fontSize = 32.sp, lineHeight = 38.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-.8).sp)
            Text("Системный звук компьютера — на твоём телефоне.", color = Muted, style = MaterialTheme.typography.bodyMedium)
        }
        Surface(shape = RoundedCornerShape(24.dp), color = Color.White, modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Box(Modifier.size(102.dp).background(Soft, CircleShape), contentAlignment = Alignment.Center) {
                    WaveMark(Modifier.size(46.dp), if (connected) Green else Color(0xFF8FA48E))
                }
                Text(when { !running -> "Приёмник выключен"; connected -> "Подключён к компьютеру";
                    ready -> "Готов к подключению"; else -> "Готовим подключение" },
                    style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                Text(when { !running -> "Включи приёмник, чтобы компьютер мог найти этот телефон."
                    connected -> "Каналы и устройства можно выбрать в RoomWave на компьютере."
                    ready -> "Выбери этот телефон в RoomWave на компьютере."
                    else -> "Подключи телефон и компьютер к одной сети Wi-Fi. Мы найдём друг друга автоматически." },
                    color = Muted, style = MaterialTheme.typography.bodyMedium, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
                Surface(color = Page, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                        Text("ЭТОТ ТЕЛЕФОН", color = Muted, fontSize = 10.sp, letterSpacing = 1.sp)
                        Text(name, fontWeight = FontWeight.Medium, style = MaterialTheme.typography.bodyLarge)
                    }
                }
                if (!running) Button(onClick = onStart, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp), shape = RoundedCornerShape(12.dp)) { Text("Включить приёмник") }
                else if (connected) OutlinedButton(onClick = onDisconnect, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp), shape = RoundedCornerShape(12.dp)) { Text("Отключиться от компьютера") }
            }
        }
        if (hasError) Surface(color = Color(0xFFFFF3E1), shape = RoundedCornerShape(16.dp)) {
            Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Нужно проверить подключение", fontWeight = FontWeight.Medium, color = Color(0xFF805A2C))
                Text("Если звук не идёт, проверь сеть и подключи телефон заново на компьютере.", color = Color(0xFF805A2C), style = MaterialTheme.typography.bodySmall)
                TextButton(onClick = onDiagnostics) { Text("Посмотреть причину") }
            }
        }
        if (!connected) Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Всего два шага", fontWeight = FontWeight.SemiBold)
            SetupStep("1", "Одна сеть Wi-Fi", "Телефон и компьютер должны быть в одной сети.")
            SetupStep("2", "Выбери телефон на ПК", "Нажми «Подключить» и назначь нужный канал.")
        }
        Surface(color = Soft, shape = RoundedCornerShape(16.dp)) {
            Text(if (connected) "Можно свернуть приложение и выключить экран — звук продолжит играть. Громкость регулируется кнопками телефона."
                else "Когда подключишься, звук будет играть даже со свёрнутым приложением и выключенным экраном.",
                Modifier.padding(18.dp), color = Green, style = MaterialTheme.typography.bodySmall)
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text("Один звук. Вся комната.", Modifier.weight(1f), fontSize = 11.sp, color = Muted)
            TextButton(onClick = onDiagnostics) { Text("Диагностика", fontSize = 12.sp) }
        }
    }
}

@Composable
private fun SetupStep(number: String, heading: String, detail: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.size(28.dp).background(Soft, CircleShape), contentAlignment = Alignment.Center) { Text(number, color = Green, fontSize = 12.sp) }
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(heading, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
            Text(detail, style = MaterialTheme.typography.bodySmall, color = Muted)
        }
    }
}

@Composable
private fun DiagnosticsScreen(onBack: () -> Unit) {
    val service = AudioReceiverService.current
    val advertiser = service?.advertiser
    val latency = service?.latencyReport
    val synchronization = service?.syncReport
    val stages = service?.stageMetrics
    fun ms(value: Double?) = value?.takeIf { it.isFinite() }?.let { "%.1f мс".format(it) } ?: "—"
    Column(Modifier.fillMaxSize().safeDrawingPadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = onBack) { Text("Назад") }
            Text("Диагностика", Modifier.padding(start = 12.dp), style = MaterialTheme.typography.titleLarge)
        }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Показатели обновляются автоматически. Здесь можно проверить соединение и работу аудиовыхода.", color = Muted, style = MaterialTheme.typography.bodySmall)
            DiagnosticGroup("Подключение", listOf(
                "Телефон" to (advertiser?.deviceName ?: "—"), "Состояние" to (service?.connection ?: "Остановлен"),
                "IP-адрес" to (advertiser?.localIp ?: "—"), "Обнаружение в сети" to (advertiser?.status ?: "—"),
                "ID устройства" to (advertiser?.deviceId ?: "—")))
            DiagnosticGroup("Воспроизведение", listOf(
                "Получено пакетов" to (service?.packets ?: 0).toString(), "Пропуски" to (service?.lost ?: 0).toString(),
                "Синхронизация" to (synchronization?.status ?: "disconnected"),
                "Отклонение" to ms(synchronization?.errorMs), "Задержка звука" to ms(latency?.latencyMs),
                "Сеть (RTT)" to ms(latency?.rttMs), "Метод оценки" to (latency?.method ?: "—"),
                "Аудиовыход" to "AAudio · PCM 48 кГц · 16 бит · стерео"))
            val stageNames = mapOf("jitterMs" to "Jitter, мс", "jitterBufferMs" to "Очередь, мс",
                "latePackets" to "Опоздавшие пакеты", "underruns" to "Underrun", "captureToSendMs" to "Захват → отправка, мс",
                "networkMs" to "Сеть, мс", "audioOutputMs" to "Аудиовыход, мс", "hardwareBufferFrames" to "Аппаратный буфер, кадров")
            val stageRows = stages?.keys()?.asSequence()?.toList()?.map { key ->
                val value = stages.opt(key)
                (stageNames[key] ?: key) to when(value) { is Double -> if(value.isFinite()) "%.2f".format(value) else "—"; null -> "—"; else -> value.toString() }
            }.orEmpty()
            if (stageRows.isNotEmpty()) DiagnosticGroup("Этапы передачи", stageRows)
            Text(when (latency?.method) {
                "queue" -> "Оценка по очереди воспроизведения; задержка аудиовыхода может быть больше."
                "capture-read" -> "От получения PCM в Windows до временной метки AAudio. Задержка до захвата и акустическая задержка динамика не измерены."
                else -> "Оценка зависит от временных меток драйвера. Она не является измерением полного акустического пути."
            }, color = Muted, style = MaterialTheme.typography.bodySmall)
            listOfNotNull(service?.streamError, AudioReceiverService.lastError, advertiser?.error).distinct().forEach { error ->
                Surface(color = MaterialTheme.colorScheme.errorContainer, shape = RoundedCornerShape(12.dp)) {
                    Text(error, Modifier.padding(16.dp), color = MaterialTheme.colorScheme.onErrorContainer, style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}

@Composable
private fun DiagnosticGroup(heading: String, rows: List<Pair<String, String>>) {
    Surface(shape = RoundedCornerShape(18.dp), modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text(heading, fontWeight = FontWeight.SemiBold)
            rows.forEach { (label, value) ->
                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(label, color = Muted, style = MaterialTheme.typography.labelMedium)
                    Text(value, style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
    }
}

@Preview(showBackground = true, widthDp = 360, heightDp = 800)
@Composable
private fun ReadyPreview() { RoomWaveTheme { ReceiverScreen("Samsung S20 FE", true, true, false, false, {}, {}, {}, {}) } }

@Preview(showBackground = true, widthDp = 360, heightDp = 800)
@Composable
private fun ConnectedPreview() { RoomWaveTheme { ReceiverScreen("Samsung S20 FE", true, true, true, false, {}, {}, {}, {}) } }

@Preview(showBackground = true, widthDp = 320, heightDp = 640, fontScale = 1.3f)
@Composable
private fun StoppedPreview() { RoomWaveTheme { ReceiverScreen("Xiaomi", false, false, false, false, {}, {}, {}, {}) } }
