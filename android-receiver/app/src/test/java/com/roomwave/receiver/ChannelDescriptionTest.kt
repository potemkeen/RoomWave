package com.roomwave.receiver

import org.junit.Assert.*
import org.junit.Test

class ChannelDescriptionTest {
    @Test fun stereoAndCenterAreExplicit() {
        assertEquals("Стерео · FL + FR", channelDescription(null, true))
        assertEquals("Центр · FC", channelDescription(4, true))
    }
    @Test fun backAndSideChannelsRemainDistinct() {
        assertEquals("Задний левый · BL", channelDescription(16, true))
        assertEquals("Боковой левый · SL", channelDescription(512, true))
        assertEquals("Задний правый · BR", channelDescription(32, true))
        assertEquals("Боковой правый · SR", channelDescription(1024, true))
    }
    @Test fun unavailableAssignmentIsNotRelabeledAsStereo() {
        assertEquals("Задний левый · BL — недоступен", channelDescription(16, false))
        assertTrue(channelDescription(3, true).startsWith("Неизвестный канал"))
    }
}
