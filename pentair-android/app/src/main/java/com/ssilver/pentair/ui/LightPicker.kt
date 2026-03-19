package com.ssilver.pentair.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ssilver.pentair.data.LightState

private data class LightMode(
    val key: String,
    val brush: Brush,
)

private val lightModes = listOf(
    LightMode("swim", Brush.linearGradient(listOf(Color(0xFF0EA5E9), Color(0xFF14B8A6)))),
    LightMode("party", Brush.sweepGradient(
        listOf(
            Color(0xFFEF4444), Color(0xFFEAB308), Color(0xFF22C55E),
            Color(0xFF3B82F6), Color(0xFFA855F7), Color(0xFFEF4444),
        )
    )),
    LightMode("romantic", Brush.linearGradient(listOf(Color(0xFFEC4899), Color(0xFFF59E0B)))),
    LightMode("caribbean", Brush.linearGradient(listOf(Color(0xFF06B6D4), Color(0xFF2DD4BF)))),
    LightMode("american", Brush.linearGradient(listOf(Color(0xFFEF4444), Color(0xFFEFF6FF), Color(0xFF3B82F6)))),
    LightMode("sunset", Brush.linearGradient(listOf(Color(0xFFF97316), Color(0xFFDC2626)))),
    LightMode("royal", Brush.linearGradient(listOf(Color(0xFF7C3AED), Color(0xFF3B82F6)))),
    LightMode("blue", Brush.linearGradient(listOf(Color(0xFF3B82F6), Color(0xFF3B82F6)))),
    LightMode("green", Brush.linearGradient(listOf(Color(0xFF22C55E), Color(0xFF22C55E)))),
    LightMode("red", Brush.linearGradient(listOf(Color(0xFFEF4444), Color(0xFFEF4444)))),
    LightMode("white", Brush.radialGradient(listOf(Color(0xFFFFFFFF), Color(0xFFCBD5E1)))),
    LightMode("purple", Brush.linearGradient(listOf(Color(0xFFA855F7), Color(0xFFA855F7)))),
)

private fun getModebrush(mode: String): Brush? {
    return lightModes.find { it.key == mode }?.brush
}

@Composable
fun LightPicker(
    lights: LightState?,
    onModeSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    var expanded by remember { mutableStateOf(false) }
    val lightsOn = lights?.on == true
    val currentMode = lights?.mode

    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = modifier,
    ) {
        // Main toggle circle
        val selectorBrush = if (lightsOn && currentMode != null) {
            getModebrush(currentMode)
        } else {
            null
        }

        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier
                .size(24.dp)
                .clip(CircleShape)
                .then(
                    if (selectorBrush != null) {
                        Modifier.background(selectorBrush, CircleShape)
                    } else {
                        Modifier.background(Color.Transparent, CircleShape)
                    }
                )
                .border(
                    width = 2.dp,
                    color = if (lightsOn) Color.White.copy(alpha = 0.5f) else Color.White.copy(alpha = 0.25f),
                    shape = CircleShape,
                )
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                ) { expanded = !expanded },
        ) {
            if (!lightsOn || currentMode == null) {
                Text(
                    text = "\u23FB", // Power symbol
                    fontSize = 12.sp,
                    color = Color.White.copy(alpha = 0.35f),
                )
            }
        }

        // Expandable color swatch row
        AnimatedVisibility(
            visible = expanded,
            enter = expandHorizontally() + fadeIn(),
            exit = shrinkHorizontally() + fadeOut(),
        ) {
            Row(
                modifier = Modifier
                    .horizontalScroll(rememberScrollState())
                    .weight(1f, fill = false),
            ) {
                Spacer(Modifier.width(5.dp))

                // Off swatch
                Box(
                    contentAlignment = Alignment.Center,
                    modifier = Modifier
                        .size(20.dp)
                        .clip(CircleShape)
                        .background(Color.White.copy(alpha = 0.08f), CircleShape)
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                        ) {
                            onModeSelect("off")
                            expanded = false
                        },
                ) {
                    Text(
                        text = "\u23FB",
                        fontSize = 10.sp,
                        color = Color.White.copy(alpha = 0.4f),
                    )
                }

                // Color swatches
                lightModes.forEach { mode ->
                    Spacer(Modifier.width(5.dp))
                    val isSelected = currentMode == mode.key
                    Box(
                        modifier = Modifier
                            .size(20.dp)
                            .clip(CircleShape)
                            .background(mode.brush, CircleShape)
                            .then(
                                if (isSelected) {
                                    Modifier.border(
                                        width = 2.dp,
                                        color = Color.White,
                                        shape = CircleShape,
                                    )
                                } else {
                                    Modifier
                                }
                            )
                            .clickable(
                                interactionSource = remember { MutableInteractionSource() },
                                indication = null,
                            ) {
                                onModeSelect(mode.key)
                                expanded = false
                            },
                    )
                }
            }
        }
    }
}
