package com.ssilver.pentair.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable

private val PoolColorScheme = darkColorScheme(
    primary = Accent,
    secondary = Teal,
    background = PoolBackground,
    surface = PoolSurface,
    surfaceVariant = PoolSurfaceVariant,
    outline = PoolOutline,
    outlineVariant = PoolOutlineVariant,
    onPrimary = PoolBackground,
    onSecondary = PoolBackground,
    onBackground = TextBright,
    onSurface = TextBright,
    onSurfaceVariant = TextDim,
)

@Composable
fun PoolTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = PoolColorScheme,
        content = content,
    )
}
