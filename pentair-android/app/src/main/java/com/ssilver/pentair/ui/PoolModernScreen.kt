package com.ssilver.pentair.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ssilver.pentair.data.BodyState
import com.ssilver.pentair.data.ConnectionState
import com.ssilver.pentair.data.LightState
import com.ssilver.pentair.data.SpaState
import com.ssilver.pentair.data.SystemInfo
import com.ssilver.pentair.ui.theme.Accent
import com.ssilver.pentair.ui.theme.Gold
import com.ssilver.pentair.ui.theme.PoolBackground
import com.ssilver.pentair.ui.theme.TextBright
import com.ssilver.pentair.ui.theme.TextDim
import com.ssilver.pentair.ui.theme.TextFaint
import com.ssilver.pentair.ui.theme.Warm

@Composable
fun PoolModernScreen(
    pool: BodyState?,
    spa: SpaState?,
    lights: LightState?,
    systemInfo: SystemInfo?,
    connectionState: ConnectionState,
    spaState: String,
    onShowSettings: () -> Unit,
    onPoolSetpointClick: () -> Unit,
    onSpaStateChange: (String) -> Unit,
    onSpaSetpointClick: () -> Unit,
    onLightModeSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier.background(
            Brush.linearGradient(
                colors = listOf(
                    Color(0xFF091120),
                    Color(0xFF0B2031),
                    PoolBackground,
                ),
            )
        ),
    ) {
        LazyColumn(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            contentPadding = PaddingValues(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 24.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            item {
                ModernHeader(
                    connectionState = connectionState,
                    systemInfo = systemInfo,
                    onShowSettings = onShowSettings,
                )
            }

            item {
                BodyControlCard(
                    title = "Pool",
                    temperature = pool?.temperature,
                    setpoint = pool?.setpoint,
                    heating = pool?.heating,
                    runningLabel = if (pool?.on == true) "Running" else "Off",
                    runningColor = if (pool?.on == true) Color(0xFF7DFFD8) else TextDim,
                    onSetpointClick = onPoolSetpointClick,
                    controls = {},
                )
            }

            item {
                BodyControlCard(
                    title = "Spa",
                    temperature = spa?.temperature,
                    setpoint = spa?.setpoint,
                    heating = spa?.heating,
                    runningLabel = spaState.replaceFirstChar { it.uppercase() },
                    runningColor = if (spa?.on == true) Gold else TextDim,
                    onSetpointClick = onSpaSetpointClick,
                ) {
                    SegmentedActionRow(
                        options = listOf("Off", "Spa", "Jets"),
                        selected = spaState.replaceFirstChar { it.uppercase() },
                        onSelect = { option ->
                            onSpaStateChange(option.lowercase())
                        },
                    )
                }
            }

            item {
                LightsCard(
                    lights = lights,
                    onLightModeSelect = onLightModeSelect,
                )
            }
        }
    }
}

@Composable
private fun ModernHeader(
    connectionState: ConnectionState,
    systemInfo: SystemInfo?,
    onShowSettings: () -> Unit,
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.weight(1f),
        ) {
            ConnectionChip(connectionState = connectionState)

            if (systemInfo != null) {
                Text(
                    text = "${systemInfo.controller} \u2022 ${systemInfo.air_temperature}\u00B0",
                    fontSize = 13.sp,
                    color = TextDim,
                )
            }
        }

        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier
                .size(44.dp)
                .clip(CircleShape)
                .background(Color.White.copy(alpha = 0.08f))
                .border(1.dp, Color.White.copy(alpha = 0.10f), CircleShape)
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClick = onShowSettings,
                ),
        ) {
            Text(
                text = "\u2699",
                fontSize = 20.sp,
                color = TextBright,
            )
        }
    }
}

@Composable
private fun ConnectionChip(connectionState: ConnectionState) {
    val dotColor = when (connectionState) {
        ConnectionState.CONNECTED -> Color(0xFF7DFFD8)
        ConnectionState.DISCONNECTED -> Color(0xFFFF7A7A)
        ConnectionState.DISCOVERING -> Color(0xFF8EC5FF)
    }
    val label = when (connectionState) {
        ConnectionState.CONNECTED -> "Connected"
        ConnectionState.DISCONNECTED -> "Disconnected"
        ConnectionState.DISCOVERING -> "Searching"
    }

    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(Color.White.copy(alpha = 0.08f))
            .border(1.dp, Color.White.copy(alpha = 0.10f), RoundedCornerShape(999.dp))
            .padding(horizontal = 14.dp, vertical = 9.dp),
    ) {
        Box(
            modifier = Modifier
                .size(10.dp)
                .clip(CircleShape)
                .background(dotColor),
        )

        Text(
            text = label,
            fontSize = 13.sp,
            fontWeight = FontWeight.SemiBold,
            color = TextBright,
        )
    }
}

@Composable
private fun BodyControlCard(
    title: String,
    temperature: Int?,
    setpoint: Int?,
    heating: String?,
    runningLabel: String,
    runningColor: Color,
    onSetpointClick: () -> Unit,
    controls: @Composable () -> Unit,
) {
    ModernPanelCard {
        Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = title,
                    fontSize = 18.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = TextBright,
                )

                Spacer(modifier = Modifier.weight(1f))

                StatusMiniChip(
                    text = if (heating == null || heating == "off") "Not heating" else "Heating",
                    color = if (heating == null || heating == "off") TextDim else Warm,
                )
                Spacer(modifier = Modifier.size(8.dp))
                StatusMiniChip(
                    text = runningLabel,
                    color = runningColor,
                )
            }

            Row(
                verticalAlignment = Alignment.Bottom,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = temperature?.let { "$it\u00B0" } ?: "--\u00B0",
                    fontSize = 46.sp,
                    lineHeight = 46.sp,
                    fontWeight = FontWeight.Bold,
                    color = TextBright,
                )
                Spacer(modifier = Modifier.weight(1f))
            }

            SetpointRow(
                label = "Setpoint",
                value = setpoint?.let { "$it\u00B0" } ?: "--",
                onClick = onSetpointClick,
            )

            controls()
        }
    }
}

@Composable
private fun StatusMiniChip(text: String, color: Color) {
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(color.copy(alpha = 0.14f))
            .border(1.dp, color.copy(alpha = 0.28f), RoundedCornerShape(999.dp))
            .padding(horizontal = 10.dp, vertical = 6.dp),
    ) {
        Text(
            text = text,
            fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold,
            color = color,
        )
    }
}

@Composable
private fun SetpointRow(
    label: String,
    value: String,
    onClick: () -> Unit,
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(Color.White.copy(alpha = 0.07f))
            .border(1.dp, Color.White.copy(alpha = 0.08f), RoundedCornerShape(18.dp))
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = onClick,
            )
            .padding(horizontal = 16.dp, vertical = 14.dp),
    ) {
        Text(
            text = label,
            fontSize = 14.sp,
            fontWeight = FontWeight.Medium,
            color = TextBright,
        )

        Spacer(modifier = Modifier.weight(1f))

        Text(
            text = value,
            fontSize = 16.sp,
            fontWeight = FontWeight.SemiBold,
            color = TextBright,
        )
    }
}

@Composable
private fun SegmentedActionRow(
    options: List<String>,
    selected: String,
    onSelect: (String) -> Unit,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        options.forEach { option ->
            val isSelected = selected == option
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(14.dp))
                    .background(
                        if (isSelected) Accent
                        else Color.White.copy(alpha = 0.08f)
                    )
                    .border(
                        width = 1.dp,
                        color = if (isSelected) Accent else Color.White.copy(alpha = 0.08f),
                        shape = RoundedCornerShape(14.dp),
                    )
                    .clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                    ) { onSelect(option) }
                    .padding(vertical = 12.dp),
            ) {
                Text(
                    text = option,
                    fontSize = 14.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = if (isSelected) PoolBackground else TextBright,
                )
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun LightsCard(
    lights: LightState?,
    onLightModeSelect: (String) -> Unit,
) {
    ModernPanelCard {
        Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = "Lights",
                    fontSize = 18.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = TextBright,
                )
                Spacer(modifier = Modifier.weight(1f))
                Text(
                    text = lightsStatusTitle(lights),
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Medium,
                    color = if (lights?.on == true) TextBright else TextDim,
                )
            }

            if (lights == null) {
                Text(
                    text = "Waiting for the light controller to report in.",
                    fontSize = 13.sp,
                    color = TextDim,
                )
            } else {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                    maxItemsInEachRow = 5,
                ) {
                    LightModeSwatch(
                        label = "Off",
                        brush = null,
                        selected = !lights.on,
                    ) {
                        onLightModeSelect("off")
                    }

                    selectableLightModes(lights).forEach { mode ->
                        LightModeSwatch(
                            label = lightModeLabel(mode),
                            brush = lightModeBrush(mode),
                            selected = lights.on && lights.mode == mode,
                        ) {
                            onLightModeSelect(mode)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun LightModeSwatch(
    label: String,
    brush: Brush?,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier
            .width(56.dp)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = onClick,
            ),
    ) {
        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier
                .size(42.dp)
                .clip(CircleShape)
                .background(brush ?: Brush.linearGradient(listOf(Color(0xFF0A0A0A), Color(0xFF0A0A0A))))
                .border(
                    width = if (selected) 2.dp else 1.dp,
                    color = if (selected) Accent else Color.White.copy(alpha = 0.14f),
                    shape = CircleShape,
                ),
        ) {
            if (brush == null) {
                Text(
                    text = "\u25CF",
                    fontSize = 12.sp,
                    color = TextDim,
                )
            }
        }

        Text(
            text = label,
            fontSize = 11.sp,
            lineHeight = 12.sp,
            fontWeight = FontWeight.Medium,
            color = if (selected) TextBright else TextDim,
        )
    }
}

@Composable
private fun ModernPanelCard(content: @Composable ColumnScope.() -> Unit) {
    Column(
        verticalArrangement = Arrangement.spacedBy(0.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(28.dp))
            .background(Color.Black.copy(alpha = 0.20f))
            .border(1.dp, Color.White.copy(alpha = 0.08f), RoundedCornerShape(28.dp))
            .padding(18.dp),
        content = content,
    )
}

private fun lightsStatusTitle(lights: LightState?): String {
    if (lights == null) return "Waiting"
    if (!lights.on) return "Off"
    return lightModeLabel(lights.mode ?: "on")
}

private fun lightModeLabel(raw: String): String = when (raw) {
    "swim" -> "Color Swim"
    "set" -> "Color Set"
    "sync" -> "Sync"
    else -> raw.replace("-", " ").replace("_", " ").replaceFirstChar { it.uppercase() }
}

private fun selectableLightModes(lights: LightState): List<String> {
    val preferredOrder = listOf(
        "swim",
        "party",
        "romantic",
        "caribbean",
        "american",
        "sunset",
        "royal",
        "blue",
        "green",
        "red",
        "white",
        "purple",
    )

    val available = lights.available_modes.toSet()
    return preferredOrder.filter { it in available }
}

private fun lightModeBrush(mode: String): Brush = when (mode) {
    "swim" -> Brush.sweepGradient(
        listOf(
            Color(0xFF0EA5E9),
            Color(0xFFEFF6FF),
            Color(0xFF1D4ED8),
            Color(0xFF14B8A6),
            Color(0xFF0EA5E9),
        )
    )
    "party" -> Brush.sweepGradient(
        listOf(
            Color(0xFFEF4444),
            Color(0xFFEAB308),
            Color(0xFF22C55E),
            Color(0xFF3B82F6),
            Color(0xFFA855F7),
            Color(0xFFEF4444),
        )
    )
    "romantic" -> Brush.linearGradient(listOf(Color(0xFFEC4899), Color(0xFFF59E0B)))
    "caribbean" -> Brush.linearGradient(listOf(Color(0xFF06B6D4), Color(0xFF2DD4BF)))
    "american" -> Brush.linearGradient(listOf(Color(0xFFEF4444), Color(0xFFEFF6FF), Color(0xFF3B82F6)))
    "sunset" -> Brush.linearGradient(listOf(Color(0xFFF97316), Color(0xFFDC2626)))
    "royal" -> Brush.linearGradient(listOf(Color(0xFF7C3AED), Color(0xFF3B82F6)))
    "blue" -> Brush.linearGradient(listOf(Color(0xFF3B82F6), Color(0xFF3B82F6)))
    "green" -> Brush.linearGradient(listOf(Color(0xFF22C55E), Color(0xFF22C55E)))
    "red" -> Brush.linearGradient(listOf(Color(0xFFEF4444), Color(0xFFEF4444)))
    "white" -> Brush.radialGradient(listOf(Color(0xFFFFFFFF), Color(0xFFCBD5E1)))
    "purple" -> Brush.linearGradient(listOf(Color(0xFFA855F7), Color(0xFFA855F7)))
    else -> Brush.linearGradient(listOf(Accent, Accent))
}
