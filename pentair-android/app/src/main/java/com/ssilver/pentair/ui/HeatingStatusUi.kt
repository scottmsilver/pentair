package com.ssilver.pentair.ui

import com.ssilver.pentair.data.BodyState
import com.ssilver.pentair.data.SpaState

enum class HeatingStatusTone {
    HEATING,
    NEUTRAL,
    WARNING,
    ERROR,
}

data class HeatingStatusUi(
    val label: String,
    val tone: HeatingStatusTone,
)

fun poolHeatingStatus(
    pool: BodyState,
    spa: SpaState?,
    sharedPump: Boolean,
): HeatingStatusUi = resolveHeatingStatus(
    on = pool.on,
    active = pool.active,
    temperature = pool.temperature,
    setpoint = pool.setpoint,
    heatMode = pool.heat_mode,
    heating = pool.heating,
    other = spa?.toOtherBodyStatus("Spa"),
    sharedPump = sharedPump,
)

fun spaHeatingStatus(
    spa: SpaState,
    pool: BodyState?,
    sharedPump: Boolean,
): HeatingStatusUi = resolveHeatingStatus(
    on = spa.on,
    active = spa.active,
    temperature = spa.temperature,
    setpoint = spa.setpoint,
    heatMode = spa.heat_mode,
    heating = spa.heating,
    other = pool?.toOtherBodyStatus("Pool"),
    sharedPump = sharedPump,
)

private data class OtherBodyStatus(
    val name: String,
    val on: Boolean,
    val active: Boolean,
    val temperature: Int,
    val setpoint: Int,
    val heatMode: String,
    val heating: String,
)

private fun BodyState.toOtherBodyStatus(name: String) = OtherBodyStatus(
    name = name,
    on = on,
    active = active,
    temperature = temperature,
    setpoint = setpoint,
    heatMode = heat_mode,
    heating = heating,
)

private fun SpaState.toOtherBodyStatus(name: String) = OtherBodyStatus(
    name = name,
    on = on,
    active = active,
    temperature = temperature,
    setpoint = setpoint,
    heatMode = heat_mode,
    heating = heating,
)

private fun resolveHeatingStatus(
    on: Boolean,
    active: Boolean,
    temperature: Int,
    setpoint: Int,
    heatMode: String,
    heating: String,
    other: OtherBodyStatus?,
    sharedPump: Boolean,
): HeatingStatusUi {
    val normalizedHeating = heating.lowercase()
    val normalizedHeatMode = heatMode.lowercase()

    if (normalizedHeating != "off" && normalizedHeating != "unknown") {
        return HeatingStatusUi(label = "Heating", tone = HeatingStatusTone.HEATING)
    }

    if (on) {
        if (normalizedHeatMode == "off") {
            return HeatingStatusUi(label = "Heat off", tone = HeatingStatusTone.NEUTRAL)
        }

        if (temperature >= setpoint) {
            return HeatingStatusUi(label = "At temp", tone = HeatingStatusTone.NEUTRAL)
        }

        if (!active) {
            return HeatingStatusUi(label = "Waiting for flow", tone = HeatingStatusTone.WARNING)
        }

        return HeatingStatusUi(label = "Heat error", tone = HeatingStatusTone.ERROR)
    }

    if (sharedPump && other?.on == true) {
        val otherHeating = other.heating.lowercase()
        val otherHeatMode = other.heatMode.lowercase()

        if (otherHeating != "off" && otherHeating != "unknown") {
            return HeatingStatusUi(
                label = "Heating ${other.name.lowercase()}",
                tone = HeatingStatusTone.HEATING,
            )
        }

        if (!other.active) {
            return HeatingStatusUi(
                label = "${other.name} starting",
                tone = HeatingStatusTone.WARNING,
            )
        }

        if (otherHeatMode != "off" && other.temperature >= other.setpoint) {
            return HeatingStatusUi(
                label = "${other.name} at temp",
                tone = HeatingStatusTone.NEUTRAL,
            )
        }

        return HeatingStatusUi(
            label = "${other.name} on",
            tone = HeatingStatusTone.NEUTRAL,
        )
    }

    return HeatingStatusUi(label = "Off", tone = HeatingStatusTone.NEUTRAL)
}
