package com.ssilver.pentair.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.AssistChip
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.ssilver.pentair.data.BodyState
import com.ssilver.pentair.data.ConnectionState
import com.ssilver.pentair.data.LightState
import com.ssilver.pentair.data.SpaState
import com.ssilver.pentair.ui.theme.Warm

@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
@Composable
fun PoolModernScreen(
    pool: BodyState?,
    spa: SpaState?,
    lights: LightState?,
    isLoading: Boolean,
    sharedPump: Boolean,
    connectionState: ConnectionState,
    spaState: String,
    onShowSettings: () -> Unit,
    onPoolSetpointClick: () -> Unit,
    onSpaStateChange: (String) -> Unit,
    onSpaSetpointClick: () -> Unit,
    onLightModeSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Scaffold(
        modifier = modifier,
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            CenterAlignedTopAppBar(
                title = {
                    ConnectionChip(
                        connectionState = connectionState,
                        onClick = onShowSettings,
                    )
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = Color.Transparent,
                    actionIconContentColor = MaterialTheme.colorScheme.onBackground,
                ),
                actions = {
                    IconButton(onClick = onShowSettings) {
                        Icon(
                            imageVector = Icons.Outlined.Settings,
                            contentDescription = "Settings",
                        )
                    }
                },
            )
        },
    ) { innerPadding ->
        LazyColumn(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            if (isLoading) {
                item {
                    Box(
                        contentAlignment = Alignment.Center,
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(260.dp),
                    ) {
                        CircularProgressIndicator()
                    }
                }
            } else {
                if (pool != null) {
                    item {
                        BodyControlCard(
                            title = "Pool",
                            temperature = pool.temperature,
                            setpoint = pool.setpoint,
                            status = poolHeatingStatus(pool, spa, sharedPump),
                            onSetpointClick = onPoolSetpointClick,
                        )
                    }
                }

                if (spa != null) {
                    item {
                        BodyControlCard(
                            title = "Spa",
                            temperature = spa.temperature,
                            setpoint = spa.setpoint,
                            status = spaHeatingStatus(spa, pool, sharedPump),
                            onSetpointClick = onSpaSetpointClick,
                        ) {
                            SegmentedActionRow(
                                options = listOf("Off", "Spa", "Jets"),
                                selected = spaState.replaceFirstChar { it.uppercase() },
                                onSelect = { option -> onSpaStateChange(option.lowercase()) },
                            )
                        }
                    }
                }

                if (lights != null) {
                    item {
                        LightsCard(
                            lights = lights,
                            onLightModeSelect = onLightModeSelect,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun ConnectionChip(
    connectionState: ConnectionState,
    onClick: () -> Unit,
) {
    val dotColor = when (connectionState) {
        ConnectionState.CONNECTED -> Color(0xFF4ADE80)
        ConnectionState.CONNECTING -> Color(0xFFF59E0B)
        ConnectionState.DISCONNECTED -> MaterialTheme.colorScheme.error
        ConnectionState.DISCOVERING -> MaterialTheme.colorScheme.primary
    }
    val label = when (connectionState) {
        ConnectionState.CONNECTED -> "Connected"
        ConnectionState.CONNECTING -> "Connecting"
        ConnectionState.DISCONNECTED -> "Disconnected"
        ConnectionState.DISCOVERING -> "Searching"
    }

    AssistChip(
        onClick = onClick,
        label = { Text(label) },
        leadingIcon = {
            Box(
                modifier = Modifier
                    .size(10.dp)
                    .clip(CircleShape)
                    .background(dotColor),
            )
        },
    )
}

@Composable
private fun BodyControlCard(
    title: String,
    temperature: Int,
    setpoint: Int,
    status: HeatingStatusUi,
    onSetpointClick: () -> Unit,
    controls: @Composable ColumnScope.() -> Unit = {},
) {
    ElevatedCard(modifier = Modifier.fillMaxWidth()) {
        Column(
            verticalArrangement = Arrangement.spacedBy(20.dp),
            modifier = Modifier.padding(20.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                )
                Spacer(modifier = Modifier.weight(1f))
                Text(
                    text = status.label,
                    style = MaterialTheme.typography.labelLarge,
                    color = heatingStatusColor(status),
                )
            }

            Text(
                text = "$temperature\u00B0",
                style = MaterialTheme.typography.displayMedium,
                fontWeight = FontWeight.SemiBold,
            )

            SetpointRow(
                label = "Setpoint",
                value = "$setpoint\u00B0",
                onClick = onSetpointClick,
            )

            controls()
        }
    }
}

@Composable
private fun heatingStatusColor(status: HeatingStatusUi): Color = when (status.tone) {
    HeatingStatusTone.HEATING -> Warm
    HeatingStatusTone.NEUTRAL -> MaterialTheme.colorScheme.onSurfaceVariant
    HeatingStatusTone.WARNING -> Color(0xFFF59E0B)
    HeatingStatusTone.ERROR -> MaterialTheme.colorScheme.error
}

@Composable
private fun SetpointRow(
    label: String,
    value: String,
    onClick: () -> Unit,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = MaterialTheme.shapes.large,
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
    ) {
        ListItem(
            headlineContent = {
                Text(
                    text = label,
                    style = MaterialTheme.typography.titleSmall,
                )
            },
            trailingContent = {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = value,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Icon(
                        imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowRight,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            },
            colors = ListItemDefaults.colors(
                containerColor = Color.Transparent,
            ),
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SegmentedActionRow(
    options: List<String>,
    selected: String,
    onSelect: (String) -> Unit,
) {
    SingleChoiceSegmentedButtonRow(
        modifier = Modifier.fillMaxWidth(),
    ) {
        options.forEachIndexed { index, option ->
            SegmentedButton(
                selected = selected == option,
                onClick = { onSelect(option) },
                shape = SegmentedButtonDefaults.itemShape(index = index, count = options.size),
                label = { Text(option) },
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun LightsCard(
    lights: LightState,
    onLightModeSelect: (String) -> Unit,
) {
    ElevatedCard(modifier = Modifier.fillMaxWidth()) {
        Column(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            modifier = Modifier.padding(20.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = "Lights",
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                )
                Spacer(modifier = Modifier.weight(1f))
                Text(
                    text = lightsStatusTitle(lights),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
                maxItemsInEachRow = 6,
            ) {
                LightModeSwatch(
                    label = "Off",
                    brush = null,
                    selected = !lights.on,
                    onClick = { onLightModeSelect("off") },
                )

                selectableLightModes(lights).forEach { mode ->
                    LightModeSwatch(
                        label = lightModeLabel(mode),
                        brush = lightModeBrush(mode),
                        selected = lights.on && lights.mode == mode,
                        onClick = { onLightModeSelect(mode) },
                    )
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
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier
            .size(50.dp)
            .semantics { contentDescription = label }
            .clickable(onClick = onClick),
    ) {
        Surface(
            shape = CircleShape,
            color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.35f),
            tonalElevation = if (selected) 3.dp else 0.dp,
            border = BorderStroke(
                width = if (selected) 2.dp else 1.dp,
                color = if (selected) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.outlineVariant
                },
            ),
            modifier = Modifier.size(46.dp),
        ) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .fillMaxSize()
                    .padding(5.dp)
                    .clip(CircleShape)
                    .background(
                        brush ?: Brush.linearGradient(
                            listOf(
                                MaterialTheme.colorScheme.scrim,
                                MaterialTheme.colorScheme.scrim,
                            )
                        )
                    ),
            ) {
                if (brush == null) {
                    Box(
                        modifier = Modifier
                            .size(8.dp)
                            .clip(CircleShape)
                            .background(MaterialTheme.colorScheme.onSurfaceVariant),
                    )
                }
            }
        }
    }
}

private fun lightsStatusTitle(lights: LightState): String {
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
    else -> Brush.linearGradient(listOf(Color(0xFF38BDF8), Color(0xFF38BDF8)))
}
