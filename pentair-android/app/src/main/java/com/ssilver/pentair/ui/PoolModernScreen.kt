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
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.CardDefaults
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
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.platform.LocalDensity
import com.ssilver.pentair.data.BodyState
import com.ssilver.pentair.data.BodyTemperaturePresentation
import com.ssilver.pentair.data.ConnectionState
import com.ssilver.pentair.data.LightState
import com.ssilver.pentair.data.SpaState
import com.ssilver.pentair.data.temperaturePresentation
import com.ssilver.pentair.ui.theme.Warm

@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
@Composable
fun PoolModernScreen(
    pool: BodyState?,
    spa: SpaState?,
    lights: LightState?,
    isLoading: Boolean,
    isRefreshing: Boolean,
    sharedPump: Boolean,
    connectionState: ConnectionState,
    spaState: String,
    onRefresh: () -> Unit,
    onShowSettings: () -> Unit,
    onPoolSetpointClick: () -> Unit,
    onSpaStateChange: (String) -> Unit,
    onSpaSetpointClick: () -> Unit,
    onLightModeSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val pullToRefreshState = rememberPullToRefreshState()
    val density = LocalDensity.current
    val contentOffsetPx = with(density) {
        (pullToRefreshState.distanceFraction.coerceIn(0f, 1.25f) * 88.dp.toPx())
    }

    Scaffold(
        modifier = modifier,
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            TopAppBar(
                title = {},
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = Color.Transparent,
                    actionIconContentColor = MaterialTheme.colorScheme.onBackground,
                ),
                actions = {
                    IconButton(onClick = onShowSettings) {
                        BadgedBox(
                            badge = {
                                ConnectionStatusBadge(connectionState = connectionState)
                            },
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Settings,
                                contentDescription = "Settings",
                            )
                        }
                    }
                },
            )
        },
    ) { innerPadding ->
        PullToRefreshBox(
            isRefreshing = isRefreshing,
            onRefresh = onRefresh,
            state = pullToRefreshState,
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            LazyColumn(
                verticalArrangement = Arrangement.spacedBy(16.dp),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        translationY = contentOffsetPx
                    },
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
                                presentation = pool.temperaturePresentation(),
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
                                presentation = spa.temperaturePresentation(),
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
}

@Composable
private fun ConnectionStatusBadge(connectionState: ConnectionState) {
    val color = when (connectionState) {
        ConnectionState.CONNECTED -> Color(0xFF4ADE80).copy(alpha = 0.76f)
        ConnectionState.CONNECTING -> Color(0xFFF59E0B)
        ConnectionState.DISCONNECTED -> MaterialTheme.colorScheme.error
        ConnectionState.DISCOVERING -> MaterialTheme.colorScheme.primary
    }

    Badge(containerColor = color)
}

@Composable
private fun BodyControlCard(
    title: String,
    presentation: BodyTemperaturePresentation,
    setpoint: Int,
    status: HeatingStatusUi?,
    onSetpointClick: () -> Unit,
    controls: @Composable ColumnScope.() -> Unit = {},
) {
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
        elevation = CardDefaults.elevatedCardElevation(
            defaultElevation = 5.dp,
            pressedElevation = 7.dp,
        ),
    ) {
        Column(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            modifier = Modifier.padding(16.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleMedium.copy(fontSize = 17.sp),
                    fontWeight = FontWeight.SemiBold,
                )
                Spacer(modifier = Modifier.weight(1f))
                if (status != null) {
                    Text(
                        text = status.label,
                        style = MaterialTheme.typography.labelMedium.copy(fontSize = 12.sp),
                        color = heatingStatusColor(status),
                    )
                }
            }

            Column(
                verticalArrangement = Arrangement.spacedBy(0.dp),
            ) {
                TemperatureLine(
                    presentation = presentation,
                )

                presentation.detailText?.let { detail ->
                    Text(
                        text = detail,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = 6.dp),
                    )
                }
            }

            controls()

            SetpointRow(
                label = "Setpoint",
                value = "$setpoint\u00B0",
                onClick = onSetpointClick,
            )
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
private fun TemperatureLine(
    presentation: BodyTemperaturePresentation,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        Text(
            text = presentation.temperatureText,
            style = MaterialTheme.typography.displayMedium.copy(
                fontSize = 46.sp,
                fontWeight = FontWeight.SemiBold,
            ),
            color = if (presentation.isStale) {
                MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f)
            } else {
                MaterialTheme.colorScheme.onSurface
            },
            modifier = Modifier.alignByBaseline(),
        )

        presentation.staleText?.let { staleText ->
            Text(
                text = staleText,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier
                    .alignByBaseline()
                    .padding(bottom = 6.dp),
            )
        }
    }
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
                    style = MaterialTheme.typography.titleSmall.copy(fontSize = 15.sp),
                    fontWeight = FontWeight.Medium,
                )
            },
            trailingContent = {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = value,
                        style = MaterialTheme.typography.bodyLarge.copy(fontSize = 17.sp),
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
        modifier = Modifier.wrapContentWidth(),
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
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
        elevation = CardDefaults.elevatedCardElevation(
            defaultElevation = 1.dp,
            pressedElevation = 3.dp,
        ),
    ) {
        Column(
            verticalArrangement = Arrangement.spacedBy(14.dp),
            modifier = Modifier.padding(16.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = "Lights",
                    style = MaterialTheme.typography.titleMedium,
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
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
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
            .size(44.dp)
            .semantics { contentDescription = label }
            .clickable(onClick = onClick),
    ) {
        Surface(
            shape = CircleShape,
            color = if (selected) {
                MaterialTheme.colorScheme.surfaceContainerHigh
            } else {
                MaterialTheme.colorScheme.surfaceContainerLow
            },
            tonalElevation = if (selected) 2.dp else 0.dp,
            border = BorderStroke(
                width = if (selected) 2.dp else 1.dp,
                color = if (selected) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.outlineVariant
                },
            ),
            modifier = Modifier.size(42.dp),
        ) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .fillMaxSize()
                    .padding(6.dp)
                    .clip(CircleShape)
                    .alpha(if (selected) 1f else 0.76f)
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
            Color(0xFF0B6FA3),
            Color(0xFFB6E8F3),
            Color(0xFF1D4ED8),
            Color(0xFF0F8F82),
            Color(0xFF0B6FA3),
        )
    )
    "party" -> Brush.sweepGradient(
        listOf(
            Color(0xFFD9485F),
            Color(0xFFD9A400),
            Color(0xFF1D9B57),
            Color(0xFF346FD1),
            Color(0xFF8C4AD8),
            Color(0xFFD9485F),
        )
    )
    "romantic" -> Brush.linearGradient(listOf(Color(0xFFD84E8B), Color(0xFFCF7A20)))
    "caribbean" -> Brush.linearGradient(listOf(Color(0xFF0E7490), Color(0xFF1AAE9F)))
    "american" -> Brush.linearGradient(listOf(Color(0xFFD9485F), Color(0xFFE6EEF7), Color(0xFF346FD1)))
    "sunset" -> Brush.linearGradient(listOf(Color(0xFFD97706), Color(0xFFB91C1C)))
    "royal" -> Brush.linearGradient(listOf(Color(0xFF6D28D9), Color(0xFF346FD1)))
    "blue" -> Brush.linearGradient(listOf(Color(0xFF346FD1), Color(0xFF346FD1)))
    "green" -> Brush.linearGradient(listOf(Color(0xFF1D9B57), Color(0xFF1D9B57)))
    "red" -> Brush.linearGradient(listOf(Color(0xFFD9485F), Color(0xFFD9485F)))
    "white" -> Brush.radialGradient(listOf(Color(0xFFF8FAFC), Color(0xFFCBD5E1)))
    "purple" -> Brush.linearGradient(listOf(Color(0xFF8C4AD8), Color(0xFF8C4AD8)))
    else -> Brush.linearGradient(listOf(Color(0xFF2B8FC9), Color(0xFF2B8FC9)))
}
