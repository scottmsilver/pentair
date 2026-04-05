package com.ssilver.pentair.data

import com.squareup.moshi.Json
import com.squareup.moshi.JsonClass

@JsonClass(generateAdapter = true)
data class PoolSystem(
    val pool: BodyState?,
    val spa: SpaState?,
    val lights: LightState?,
    val auxiliaries: List<AuxState>,
    val pump: PumpInfo?,
    val system: SystemInfo,
    val goodnight_available: Boolean = false,
    val matter: MatterStatus? = null,
)

@JsonClass(generateAdapter = true)
data class MatterStatus(
    val commissioned: Boolean,
    @Json(name = "status_display") val statusDisplay: String,
    @Json(name = "can_reset") val canReset: Boolean,
    @Json(name = "pairing_code") val pairingCode: String? = null,
)

@JsonClass(generateAdapter = true)
data class BodyState(
    val on: Boolean,
    val active: Boolean = false,
    override val temperature: Int,
    override val temperature_reliable: Boolean = true,
    override val temperature_reason: String? = null,
    override val last_reliable_temperature: Int? = null,
    override val last_reliable_temperature_at_unix_ms: Long? = null,
    val setpoint: Int,
    val heat_mode: String,
    val heating: String,
    override val heat_estimate: HeatEstimate? = null,
    val temperature_display: TemperatureDisplay = TemperatureDisplay(null, false, null, null),
    val heat_estimate_display: HeatEstimateDisplay = HeatEstimateDisplay("unavailable", null, null, null, null),
) : TemperaturePresentationSource

@JsonClass(generateAdapter = true)
data class SpaState(
    val on: Boolean,
    val active: Boolean = false,
    override val temperature: Int,
    override val temperature_reliable: Boolean = true,
    override val temperature_reason: String? = null,
    override val last_reliable_temperature: Int? = null,
    override val last_reliable_temperature_at_unix_ms: Long? = null,
    val setpoint: Int,
    val heat_mode: String,
    val heating: String,
    override val heat_estimate: HeatEstimate? = null,
    val temperature_display: TemperatureDisplay = TemperatureDisplay(null, false, null, null),
    val heat_estimate_display: HeatEstimateDisplay = HeatEstimateDisplay("unavailable", null, null, null, null),
    val spa_heat_progress: SpaHeatProgress = SpaHeatProgress(),
    val accessories: Map<String, Boolean>
) : TemperaturePresentationSource

@JsonClass(generateAdapter = true)
data class HeatEstimate(
    val available: Boolean,
    val minutes_remaining: Int?,
    val current_temperature: Int,
    val target_temperature: Int,
    val confidence: String,
    val source: String,
    val reason: String,
    val observed_rate_per_hour: Double?,
    val learned_rate_per_hour: Double?,
    val configured_rate_per_hour: Double?,
    val baseline_rate_per_hour: Double?,
    val updated_at_unix_ms: Long,
)

@JsonClass(generateAdapter = true)
data class TemperatureDisplay(
    val value: Int?,
    val is_stale: Boolean,
    val stale_reason: String?,
    val last_reliable_at_unix_ms: Long?,
)

@JsonClass(generateAdapter = true)
data class HeatEstimateDisplay(
    val state: String,
    val reason: String?,
    val available_in_seconds: Int?,
    val minutes_remaining: Int?,
    val target_temperature: Int?,
)

@JsonClass(generateAdapter = true)
data class SpaHeatProgress(
    val active: Boolean = false,
    val phase: String = "off",
    val start_temp_f: Int? = null,
    val current_temp_f: Int = 0,
    val target_temp_f: Int = 0,
    val progress_pct: Int = 0,
    val minutes_remaining: Int? = null,
    val session_id: String? = null,
    val milestone: String? = null,
)

@JsonClass(generateAdapter = true)
data class LightState(
    val on: Boolean,
    val mode: String?,
    val available_modes: List<String>
)

@JsonClass(generateAdapter = true)
data class AuxState(
    val id: String,
    val name: String,
    val on: Boolean
)

@JsonClass(generateAdapter = true)
data class PumpInfo(
    val pump_type: String,
    val running: Boolean,
    val watts: Int,
    val rpm: Int,
    val gpm: Int
)

@JsonClass(generateAdapter = true)
data class SystemInfo(
    val controller: String,
    val firmware: String?,
    val temp_unit: String,
    val air_temperature: Int,
    val freeze_protection: Boolean,
    val pool_spa_shared_pump: Boolean
)

@JsonClass(generateAdapter = true)
data class ApiResponse(
    val ok: Boolean,
    val error: String? = null
)

interface TemperaturePresentationSource {
    val temperature: Int
    val temperature_reliable: Boolean
    val temperature_reason: String?
    val last_reliable_temperature: Int?
    val last_reliable_temperature_at_unix_ms: Long?
    val heat_estimate: HeatEstimate?
}

data class BodyTemperaturePresentation(
    val temperatureText: String,
    val staleText: String?,
    val detailText: String?,
    val isStale: Boolean,
)

fun BodyState.optimisticCommand(
    on: Boolean,
    sharedPump: Boolean,
    nowMs: Long = System.currentTimeMillis(),
): BodyState {
    val snapshot = snapshotLastReliable(nowMs)
    val nextReliable = !sharedPump
    val nextReason = when {
        !sharedPump -> null
        !on -> "inactive-shared-body"
        else -> "waiting-for-flow"
    }

    return copy(
        on = on,
        active = false,
        temperature_reliable = nextReliable,
        temperature_reason = nextReason,
        last_reliable_temperature = snapshot.first,
        last_reliable_temperature_at_unix_ms = snapshot.second,
        heat_estimate = null,
        temperature_display = TemperatureDisplay(null, false, null, null),
        heat_estimate_display = HeatEstimateDisplay("unavailable", null, null, null, null),
    )
}

fun SpaState.optimisticCommand(
    on: Boolean,
    accessories: Map<String, Boolean>,
    sharedPump: Boolean,
    nowMs: Long = System.currentTimeMillis(),
): SpaState {
    val snapshot = snapshotLastReliable(nowMs)
    val nextReliable = !sharedPump
    val nextReason = when {
        !sharedPump -> null
        !on -> "inactive-shared-body"
        else -> "waiting-for-flow"
    }

    return copy(
        on = on,
        active = false,
        temperature_reliable = nextReliable,
        temperature_reason = nextReason,
        last_reliable_temperature = snapshot.first,
        last_reliable_temperature_at_unix_ms = snapshot.second,
        heat_estimate = null,
        temperature_display = TemperatureDisplay(null, false, null, null),
        heat_estimate_display = HeatEstimateDisplay("unavailable", null, null, null, null),
        accessories = accessories,
    )
}

fun BodyState.optimisticSetpointChange(setpoint: Int): BodyState = copy(
    setpoint = setpoint,
    heat_estimate = null,
    heat_estimate_display = HeatEstimateDisplay("unavailable", null, null, null, null),
)

fun SpaState.optimisticSetpointChange(setpoint: Int): SpaState = copy(
    setpoint = setpoint,
    heat_estimate = null,
    heat_estimate_display = HeatEstimateDisplay("unavailable", null, null, null, null),
)

fun TemperaturePresentationSource.temperaturePresentation(
    nowMs: Long = System.currentTimeMillis(),
): BodyTemperaturePresentation {
    return BodyTemperaturePresentation(
        temperatureText = temperatureDisplayText(),
        staleText = staleDisplayText(nowMs),
        detailText = estimateDisplayText(),
        isStale = temperatureDisplay().is_stale,
    )
}

private fun TemperaturePresentationSource.temperatureDisplay(): TemperatureDisplay = when (this) {
    is BodyState -> {
        if (
            temperature_display.value != null ||
            temperature_display.is_stale ||
            temperature_display.stale_reason != null ||
            temperature_display.last_reliable_at_unix_ms != null
        ) {
            temperature_display
        } else {
            TemperatureDisplay(
                value = if (temperature_reliable) temperature else last_reliable_temperature,
                is_stale = !temperature_reliable,
                stale_reason = temperature_reason,
                last_reliable_at_unix_ms = last_reliable_temperature_at_unix_ms,
            )
        }
    }
    is SpaState -> {
        if (
            temperature_display.value != null ||
            temperature_display.is_stale ||
            temperature_display.stale_reason != null ||
            temperature_display.last_reliable_at_unix_ms != null
        ) {
            temperature_display
        } else {
            TemperatureDisplay(
                value = if (temperature_reliable) temperature else last_reliable_temperature,
                is_stale = !temperature_reliable,
                stale_reason = temperature_reason,
                last_reliable_at_unix_ms = last_reliable_temperature_at_unix_ms,
            )
        }
    }
    else -> TemperatureDisplay(null, false, null, null)
}

private fun TemperaturePresentationSource.heatEstimateDisplay(): HeatEstimateDisplay = when (this) {
    is BodyState -> {
        if (heat_estimate_display.state != "unavailable" || heat_estimate_display.reason != null || heat_estimate_display.available_in_seconds != null || heat_estimate_display.minutes_remaining != null) {
            heat_estimate_display
        } else {
            heat_estimate?.toDisplay() ?: HeatEstimateDisplay("unavailable", null, null, null, null)
        }
    }
    is SpaState -> {
        if (heat_estimate_display.state != "unavailable" || heat_estimate_display.reason != null || heat_estimate_display.available_in_seconds != null || heat_estimate_display.minutes_remaining != null) {
            heat_estimate_display
        } else {
            heat_estimate?.toDisplay() ?: HeatEstimateDisplay("unavailable", null, null, null, null)
        }
    }
    else -> HeatEstimateDisplay("unavailable", null, null, null, null)
}

private fun HeatEstimate.toDisplay(): HeatEstimateDisplay = when {
    available -> HeatEstimateDisplay(
        state = "ready",
        reason = null,
        available_in_seconds = null,
        minutes_remaining = minutes_remaining,
        target_temperature = target_temperature,
    )
    reason == "sensor-warmup" || reason == "insufficient-data" -> HeatEstimateDisplay(
        state = "pending",
        reason = reason,
        available_in_seconds = null,
        minutes_remaining = null,
        target_temperature = target_temperature,
    )
    else -> HeatEstimateDisplay(
        state = "unavailable",
        reason = reason,
        available_in_seconds = null,
        minutes_remaining = null,
        target_temperature = target_temperature,
    )
}

private fun TemperaturePresentationSource.temperatureDisplayText(): String {
    return temperatureDisplay().value?.let { "$it\u00B0" } ?: "--\u00B0"
}

private fun TemperaturePresentationSource.staleDisplayText(nowMs: Long): String? {
    val display = temperatureDisplay()
    val lastReliableAt = display.last_reliable_at_unix_ms
        ?: return if (display.is_stale) "Waiting for a live water temperature" else null

    return relativeTimeText(lastReliableAt, nowMs)
}

private fun TemperaturePresentationSource.estimateDisplayText(): String? {
    val display = heatEstimateDisplay()
    return when (display.state) {
        "ready" -> {
            val minutes = display.minutes_remaining ?: return null
            val target = display.target_temperature ?: return null
            "About ${formatEta(minutes)} to ${target}\u00B0"
        }
        "pending" -> when {
            display.available_in_seconds != null -> {
                if (display.available_in_seconds < 60) {
                    "Estimate in under 1 min"
                } else {
                    val roundedMinutes = kotlin.math.ceil(display.available_in_seconds / 60.0).toInt()
                    "Estimate in about $roundedMinutes min"
                }
            }
            display.reason == "insufficient-data" -> "Learning estimate"
            else -> "Generating estimate"
        }
        else -> null
    }
}

private fun relativeTimeText(unixMs: Long, nowMs: Long): String {
    val ageSeconds = ((nowMs - unixMs).coerceAtLeast(0L) / 1000L).toInt()
    if (ageSeconds < 60) {
        return "just now"
    }

    val minutes = ageSeconds / 60
    if (minutes < 60) {
        return "$minutes min ago"
    }

    val hours = (minutes + 30) / 60
    if (hours < 24) {
        return if (hours == 1) "1h ago" else "${hours}h ago"
    }

    if (hours < 48) {
        return "yesterday"
    }

    val days = (hours + 12) / 24
    return "${days}d ago"
}

private fun formatEta(minutes: Int): String {
    if (minutes < 60) {
        return "$minutes min"
    }

    val hours = minutes / 60
    val remainingMinutes = minutes % 60
    if (remainingMinutes == 0) {
        return if (hours == 1) "1 hr" else "$hours hr"
    }

    val hoursText = if (hours == 1) "1 hr" else "$hours hr"
    return "$hoursText $remainingMinutes min"
}

private fun TemperaturePresentationSource.snapshotLastReliable(nowMs: Long): Pair<Int?, Long?> {
    val display = temperatureDisplay()
    val snapshotTemperature = if (temperature_reliable) {
        temperature
    } else {
        display.value ?: last_reliable_temperature
    }

    val snapshotTime = if (temperature_reliable) {
        nowMs
    } else {
        display.last_reliable_at_unix_ms ?: last_reliable_temperature_at_unix_ms
    }

    return snapshotTemperature to snapshotTime
}
