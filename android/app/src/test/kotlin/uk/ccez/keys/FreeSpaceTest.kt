package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uk.ccez.keys.overlay.FreeSpace
import uk.ccez.keys.overlay.FreeSpace.Box

class FreeSpaceTest {
    private val screen = Box(0, 0, 1200, 2400)

    @Test fun wordsGoOnlyWhereNothingIs() {
        // Text down the left column of a 3 × 4 grid: no cell touching it.
        val cells = FreeSpace.freeCells(screen, listOf(Box(0, 0, 400, 2400)), 3, 4)
        assertEquals(8, cells.size)
        assertTrue(cells.all { it.left >= 400 })
        // Everything covered: nowhere. Nothing covered: everywhere.
        assertTrue(FreeSpace.freeCells(screen, listOf(screen), 3, 4).isEmpty())
        assertEquals(12, FreeSpace.freeCells(screen, emptyList(), 3, 4).size)
    }

    @Test fun contentByKindNotByText() {
        assertTrue(FreeSpace.isContentClass("android.widget.TextView"))
        assertTrue(FreeSpace.isContentClass("com.example.FancyImageView"))
        assertTrue(FreeSpace.isVideoClass("com.google.android.exoplayer2.ui.PlayerView"))
        assertFalse(FreeSpace.isContentClass("android.widget.LinearLayout"))
        assertFalse(FreeSpace.isContentClass(null))
    }

    @Test fun theScreenshotPanelIsKnown() {
        assertTrue(FreeSpace.isScreenshotWindow("com.android.systemui", "Screenshot", null, emptyList()))
        assertTrue(FreeSpace.isScreenshotWindow("com.android.systemui", null, "com.android.systemui.screenshot.ScreenshotController", emptyList()))
        assertTrue(FreeSpace.isScreenshotWindow("com.android.systemui", null, null, listOf("com.android.systemui:id/screenshot_actions")))
        assertFalse(FreeSpace.isScreenshotWindow("com.android.systemui", "Notifications", null, emptyList()))
        assertFalse(FreeSpace.isScreenshotWindow("com.example.app", null, null, emptyList()))
    }
}
