package com.roomwave.receiver

internal fun channelDescription(speaker: Int?, available: Boolean): String {
    if (speaker == null) return "Стерео · FL + FR"
    val name = when (speaker) {
        1 -> "Передний левый · FL"
        2 -> "Передний правый · FR"
        4 -> "Центр · FC"
        8 -> "Сабвуфер · LFE"
        16 -> "Задний левый · BL"
        32 -> "Задний правый · BR"
        64 -> "Передний левый центральный · FLC"
        128 -> "Передний правый центральный · FRC"
        256 -> "Задний центральный · BC"
        512 -> "Боковой левый · SL"
        1024 -> "Боковой правый · SR"
        else -> "Неизвестный канал ($speaker)"
    }
    return if (available) name else "$name — недоступен"
}
