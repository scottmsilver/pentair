package com.ssilver.pentair.widget

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.GlanceTheme
import androidx.glance.action.actionStartActivity
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.currentState
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.width
import androidx.glance.state.GlanceStateDefinition
import androidx.glance.state.PreferencesGlanceStateDefinition
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import com.ssilver.pentair.MainActivity

/**
 * Glance AppWidget showing spa heating progress with a rich visual design
 * matching the iOS Live Activity aesthetic.
 */
class SpaHeatWidget : GlanceAppWidget() {

    override val stateDefinition: GlanceStateDefinition<*> = PreferencesGlanceStateDefinition

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        provideContent {
            GlanceTheme {
                val prefs = currentState<Preferences>()
                val active = prefs[PrefKeys.ACTIVE] ?: false
                val phase = prefs[PrefKeys.PHASE] ?: "off"

                Box(
                    modifier = GlanceModifier
                        .fillMaxSize()
                        .cornerRadius(16.dp)
                        .background(WidgetColors.Background)
                        .clickable(actionStartActivity<MainActivity>())
                        .padding(16.dp),
                ) {
                    when {
                        phase == "reached" -> ReachedContent(prefs)
                        active -> HeatingContent(prefs)
                        else -> IdleContent(prefs)
                    }
                }
            }
        }
    }

    companion object PrefKeys {
        val ACTIVE = booleanPreferencesKey("active")
        val PHASE = stringPreferencesKey("phase")
        val CURRENT_TEMP = intPreferencesKey("current_temp_f")
        val TARGET_TEMP = intPreferencesKey("target_temp_f")
        val PROGRESS_PCT = intPreferencesKey("progress_pct")
        val MINUTES_REMAINING = intPreferencesKey("minutes_remaining")
        val HAS_MINUTES_REMAINING = booleanPreferencesKey("has_minutes_remaining")
        val POOL_TEMP = intPreferencesKey("pool_temp")
        val SPA_TEMP = intPreferencesKey("spa_temp")
        val SPA_HEAT_MODE = stringPreferencesKey("spa_heat_mode")
        val POOL_HEAT_MODE = stringPreferencesKey("pool_heat_mode")
    }
}

// ---- Color constants matching iOS Live Activity and notification palette ----

private object WidgetColors {
    val Background = androidx.glance.color.ColorProvider(
        day = android.graphics.Color.valueOf(0f, 0f, 0f, 0.85f).toArgb().let {
            androidx.compose.ui.graphics.Color(it)
        },
        night = android.graphics.Color.valueOf(0f, 0f, 0f, 0.85f).toArgb().let {
            androidx.compose.ui.graphics.Color(it)
        },
    )
    val DeepOrange = ColorProvider(androidx.compose.ui.graphics.Color(0xFFFF6D00))
    val Orange = ColorProvider(androidx.compose.ui.graphics.Color(0xFFFF9100))
    val Amber = ColorProvider(androidx.compose.ui.graphics.Color(0xFFFFC107))
    val Green = ColorProvider(androidx.compose.ui.graphics.Color(0xFF4CAF50))
    val DarkGray = ColorProvider(androidx.compose.ui.graphics.Color(0xFF424242))
    val White = ColorProvider(androidx.compose.ui.graphics.Color(0xFFFFFFFF))
    val Secondary = ColorProvider(androidx.compose.ui.graphics.Color(0xFF9E9E9E))
    val LightGray = ColorProvider(androidx.compose.ui.graphics.Color(0xFF757575))
}

private fun progressColor(pct: Int): ColorProvider = when {
    pct < 30 -> WidgetColors.DeepOrange
    pct < 70 -> WidgetColors.Orange
    pct < 90 -> WidgetColors.Amber
    else -> WidgetColors.Green
}

private fun etaText(minutes: Int?): String = when {
    minutes == null -> "Heating..."
    minutes < 60 -> "~$minutes min"
    else -> {
        val h = minutes / 60
        val m = minutes % 60
        if (m == 0) "~${h} hr" else "~${h} hr $m min"
    }
}

// ---- Heating state: shows progress ----

@Composable
private fun HeatingContent(prefs: Preferences) {
    val currentTemp = prefs[SpaHeatWidget.CURRENT_TEMP] ?: 0
    val targetTemp = prefs[SpaHeatWidget.TARGET_TEMP] ?: 0
    val progressPct = (prefs[SpaHeatWidget.PROGRESS_PCT] ?: 0).coerceIn(0, 100)
    val hasMinutes = prefs[SpaHeatWidget.HAS_MINUTES_REMAINING] ?: false
    val minutesRemaining = if (hasMinutes) prefs[SpaHeatWidget.MINUTES_REMAINING] else null

    Column(modifier = GlanceModifier.fillMaxSize()) {
        // Title row: flame + "Spa Heating" + ETA
        Row(
            modifier = GlanceModifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "\uD83D\uDD25 Spa Heating",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
            Spacer(modifier = GlanceModifier.defaultWeight())
            Text(
                text = etaText(minutesRemaining),
                style = TextStyle(
                    color = WidgetColors.Orange,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Medium,
                ),
            )
        }

        Spacer(modifier = GlanceModifier.height(8.dp))

        // Temperature row: current -> target
        Row(
            modifier = GlanceModifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "$currentTemp\u00B0F",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 28.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
            Spacer(modifier = GlanceModifier.defaultWeight())
            Text(
                text = "\u2192",
                style = TextStyle(
                    color = WidgetColors.Secondary,
                    fontSize = 16.sp,
                ),
            )
            Spacer(modifier = GlanceModifier.defaultWeight())
            Text(
                text = "$targetTemp\u00B0F",
                style = TextStyle(
                    color = WidgetColors.Secondary,
                    fontSize = 28.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
        }

        Spacer(modifier = GlanceModifier.height(8.dp))

        // Progress bar
        ProgressBar(progressPct)

        Spacer(modifier = GlanceModifier.height(4.dp))

        // Percentage text
        if (progressPct > 0) {
            Text(
                text = "$progressPct%",
                style = TextStyle(
                    color = WidgetColors.LightGray,
                    fontSize = 13.sp,
                ),
            )
        }
    }
}

// ---- Reached state: green accent ----

@Composable
private fun ReachedContent(prefs: Preferences) {
    val targetTemp = prefs[SpaHeatWidget.TARGET_TEMP] ?: 0

    Column(modifier = GlanceModifier.fillMaxSize()) {
        // Title row
        Row(
            modifier = GlanceModifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "\uD83D\uDD25 Spa Heating",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
            Spacer(modifier = GlanceModifier.defaultWeight())
            Text(
                text = "Ready!",
                style = TextStyle(
                    color = WidgetColors.Green,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
        }

        Spacer(modifier = GlanceModifier.height(8.dp))

        // Checkmark + ready text
        Row(
            modifier = GlanceModifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "\u2705 Spa is ready!",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 28.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
            Spacer(modifier = GlanceModifier.defaultWeight())
            Text(
                text = "$targetTemp\u00B0F",
                style = TextStyle(
                    color = WidgetColors.Green,
                    fontSize = 28.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
        }

        Spacer(modifier = GlanceModifier.height(8.dp))

        // Full green progress bar
        ProgressBar(100)
    }
}

// ---- Idle state: compact pool/spa summary ----

@Composable
private fun IdleContent(prefs: Preferences) {
    val poolTemp = prefs[SpaHeatWidget.POOL_TEMP] ?: 0
    val spaTemp = prefs[SpaHeatWidget.SPA_TEMP] ?: 0
    val spaHeatMode = prefs[SpaHeatWidget.SPA_HEAT_MODE] ?: "off"
    val poolHeatMode = prefs[SpaHeatWidget.POOL_HEAT_MODE] ?: "off"

    Column(
        modifier = GlanceModifier.fillMaxSize(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            modifier = GlanceModifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "\uD83C\uDFCA Pool",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
            Spacer(modifier = GlanceModifier.width(8.dp))
            Text(
                text = if (poolTemp > 0) "$poolTemp\u00B0F" else "--\u00B0F",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 22.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
            Spacer(modifier = GlanceModifier.defaultWeight())
            Text(
                text = "\u2668\uFE0F Spa",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
            Spacer(modifier = GlanceModifier.width(8.dp))
            Text(
                text = if (spaTemp > 0) "$spaTemp\u00B0F" else "--\u00B0F",
                style = TextStyle(
                    color = WidgetColors.White,
                    fontSize = 22.sp,
                    fontWeight = FontWeight.Bold,
                ),
            )
        }

        Spacer(modifier = GlanceModifier.height(4.dp))

        // Status line
        val statusParts = mutableListOf<String>()
        if (spaHeatMode != "off") statusParts.add("Spa: $spaHeatMode")
        if (poolHeatMode != "off") statusParts.add("Pool: $poolHeatMode")
        val statusText = if (statusParts.isEmpty()) "Idle" else statusParts.joinToString(" \u2022 ")

        Text(
            text = statusText,
            style = TextStyle(
                color = WidgetColors.Secondary,
                fontSize = 13.sp,
            ),
        )
    }
}

// ---- Custom progress bar using segmented boxes ----
// Glance does not support fractional widths, so we render SEGMENT_COUNT
// equal-weight boxes, each colored according to its position relative to
// the progress percentage.

private const val SEGMENT_COUNT = 20

@Composable
private fun ProgressBar(progressPct: Int) {
    val pct = progressPct.coerceIn(0, 100)
    val filledSegments = (pct * SEGMENT_COUNT) / 100
    val filledColor = progressColor(pct)
    val barHeight = 8.dp

    Row(
        modifier = GlanceModifier
            .fillMaxWidth()
            .height(barHeight)
            .cornerRadius(4.dp)
            .background(WidgetColors.DarkGray),
    ) {
        for (i in 0 until SEGMENT_COUNT) {
            val color = if (i < filledSegments) filledColor else WidgetColors.DarkGray
            Box(
                modifier = GlanceModifier
                    .height(barHeight)
                    .defaultWeight()
                    .background(color),
            ) {}
        }
    }
}

// ---- Widget receiver (entry point for the system) ----

class SpaHeatWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = SpaHeatWidget()
}
