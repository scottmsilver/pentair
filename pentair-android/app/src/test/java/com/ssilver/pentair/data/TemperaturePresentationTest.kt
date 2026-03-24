package com.ssilver.pentair.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TemperaturePresentationTest {
    @Test
    fun `stale body rounds hour-scale age text`() {
        val body = BodyState(
            on = false,
            active = false,
            temperature = 104,
            temperature_reliable = false,
            temperature_reason = "inactive-shared-body",
            last_reliable_temperature = 100,
            last_reliable_temperature_at_unix_ms = 1_000L,
            setpoint = 102,
            heat_mode = "off",
            heating = "off",
        )

        val presentation = body.temperaturePresentation(nowMs = 4_081_000L)

        assertEquals("100°", presentation.temperatureText)
        assertEquals("1h ago", presentation.staleText)
        assertNull(presentation.detailText)
        assertEquals(true, presentation.isStale)
    }

    @Test
    fun `warming body shows generating estimate while eta is unavailable`() {
        val spa = SpaState(
            on = true,
            active = true,
            temperature = 86,
            temperature_reliable = false,
            temperature_reason = "sensor-warmup",
            last_reliable_temperature = 100,
            last_reliable_temperature_at_unix_ms = 1_000L,
            setpoint = 102,
            heat_mode = "heater",
            heating = "heater",
            heat_estimate_display = HeatEstimateDisplay(
                state = "pending",
                reason = "sensor-warmup",
                available_in_seconds = 120,
                minutes_remaining = null,
                target_temperature = 102,
            ),
            heat_estimate = HeatEstimate(
                available = false,
                minutes_remaining = null,
                current_temperature = 86,
                target_temperature = 102,
                confidence = "none",
                source = "none",
                reason = "sensor-warmup",
                observed_rate_per_hour = null,
                learned_rate_per_hour = null,
                configured_rate_per_hour = null,
                baseline_rate_per_hour = null,
                updated_at_unix_ms = 10_000L,
            ),
            accessories = emptyMap(),
        )

        val presentation = spa.temperaturePresentation(nowMs = 10_000L)

        assertEquals("100°", presentation.temperatureText)
        assertEquals("just now", presentation.staleText)
        assertEquals("Estimate in about 2 min", presentation.detailText)
        assertEquals(true, presentation.isStale)
    }

    @Test
    fun `recent stale body still shows relative age text`() {
        val body = BodyState(
            on = false,
            active = false,
            temperature = 104,
            temperature_reliable = false,
            temperature_reason = "inactive-shared-body",
            last_reliable_temperature = 100,
            last_reliable_temperature_at_unix_ms = 60_000L,
            setpoint = 102,
            heat_mode = "off",
            heating = "off",
        )

        val presentation = body.temperaturePresentation(nowMs = 360_000L)

        assertEquals("100°", presentation.temperatureText)
        assertEquals("5 min ago", presentation.staleText)
        assertNull(presentation.detailText)
        assertEquals(true, presentation.isStale)
    }

    @Test
    fun `insufficient data pending state shows learning estimate`() {
        val spa = SpaState(
            on = true,
            active = true,
            temperature = 90,
            temperature_reliable = true,
            temperature_reason = null,
            last_reliable_temperature = 90,
            last_reliable_temperature_at_unix_ms = 10_000L,
            setpoint = 102,
            heat_mode = "heater",
            heating = "heater",
            heat_estimate_display = HeatEstimateDisplay(
                state = "pending",
                reason = "insufficient-data",
                available_in_seconds = null,
                minutes_remaining = null,
                target_temperature = 102,
            ),
            accessories = emptyMap(),
        )

        val presentation = spa.temperaturePresentation(nowMs = 70_000L)

        assertEquals("90°", presentation.temperatureText)
        assertEquals("1 min ago", presentation.staleText)
        assertEquals("Learning estimate", presentation.detailText)
        assertEquals(false, presentation.isStale)
    }

    @Test
    fun `day scale age text uses yesterday near one day`() {
        val body = BodyState(
            on = false,
            active = false,
            temperature = 104,
            temperature_reliable = false,
            temperature_reason = "inactive-shared-body",
            last_reliable_temperature = 100,
            last_reliable_temperature_at_unix_ms = 0L,
            setpoint = 102,
            heat_mode = "off",
            heating = "off",
        )

        val presentation = body.temperaturePresentation(nowMs = 25L * 60 * 60 * 1000)

        assertEquals("100°", presentation.temperatureText)
        assertEquals("yesterday", presentation.staleText)
        assertNull(presentation.detailText)
        assertEquals(true, presentation.isStale)
    }

    @Test
    fun `sixty eight minutes rounds to one hour`() {
        val body = BodyState(
            on = false,
            active = false,
            temperature = 104,
            temperature_reliable = false,
            temperature_reason = "inactive-shared-body",
            last_reliable_temperature = 100,
            last_reliable_temperature_at_unix_ms = 1_000L,
            setpoint = 102,
            heat_mode = "off",
            heating = "off",
        )

        val presentation = body.temperaturePresentation(nowMs = 4_081_000L)

        assertEquals("100°", presentation.temperatureText)
        assertEquals("1h ago", presentation.staleText)
        assertNull(presentation.detailText)
    }

    @Test
    fun `two days rounds to day summary`() {
        val body = BodyState(
            on = false,
            active = false,
            temperature = 104,
            temperature_reliable = false,
            temperature_reason = "inactive-shared-body",
            last_reliable_temperature = 100,
            last_reliable_temperature_at_unix_ms = 0L,
            setpoint = 102,
            heat_mode = "off",
            heating = "off",
        )

        val presentation = body.temperaturePresentation(nowMs = 49L * 60 * 60 * 1000)

        assertEquals("100°", presentation.temperatureText)
        assertEquals("2d ago", presentation.staleText)
        assertNull(presentation.detailText)
    }

    @Test
    fun `reliable body with eta shows estimate detail`() {
        val body = BodyState(
            on = true,
            active = true,
            temperature = 95,
            temperature_reliable = true,
            setpoint = 102,
            heat_mode = "heater",
            heating = "heater",
            heat_estimate = HeatEstimate(
                available = true,
                minutes_remaining = 13,
                current_temperature = 95,
                target_temperature = 102,
                confidence = "medium",
                source = "learned",
                reason = "ok",
                observed_rate_per_hour = null,
                learned_rate_per_hour = 8.0,
                configured_rate_per_hour = 9.0,
                baseline_rate_per_hour = 8.5,
                updated_at_unix_ms = 10_000L,
            ),
        )

        val presentation = body.temperaturePresentation(nowMs = 10_000L)

        assertEquals("95°", presentation.temperatureText)
        assertNull(presentation.staleText)
        assertEquals("About 13 min to 102°", presentation.detailText)
        assertEquals(false, presentation.isStale)
    }

    @Test
    fun `reliable body still shows freshness age text`() {
        val body = BodyState(
            on = true,
            active = true,
            temperature = 95,
            temperature_reliable = true,
            last_reliable_temperature = 95,
            last_reliable_temperature_at_unix_ms = 10_000L,
            setpoint = 102,
            heat_mode = "heater",
            heating = "heater",
        )

        val presentation = body.temperaturePresentation(nowMs = 40_000L)

        assertEquals("95°", presentation.temperatureText)
        assertEquals("just now", presentation.staleText)
        assertNull(presentation.detailText)
        assertEquals(false, presentation.isStale)
    }

    @Test
    fun `optimistic shared spa command clears stale daemon estimate contract`() {
        val spa = SpaState(
            on = true,
            active = true,
            temperature = 95,
            temperature_reliable = true,
            setpoint = 102,
            heat_mode = "heater",
            heating = "heater",
            heat_estimate = HeatEstimate(
                available = true,
                minutes_remaining = 13,
                current_temperature = 95,
                target_temperature = 102,
                confidence = "medium",
                source = "learned",
                reason = "estimating",
                observed_rate_per_hour = null,
                learned_rate_per_hour = 8.0,
                configured_rate_per_hour = 9.0,
                baseline_rate_per_hour = 8.5,
                updated_at_unix_ms = 10_000L,
            ),
            heat_estimate_display = HeatEstimateDisplay(
                state = "ready",
                reason = null,
                available_in_seconds = null,
                minutes_remaining = 13,
                target_temperature = 102,
            ),
            accessories = mapOf("jets" to true),
        )

        val next = spa.optimisticCommand(
            on = false,
            accessories = emptyMap(),
            sharedPump = true,
            nowMs = 60_000L,
        )

        val presentation = next.temperaturePresentation(nowMs = 60_000L)

        assertEquals(false, next.on)
        assertEquals(false, next.active)
        assertEquals(false, next.temperature_reliable)
        assertEquals("inactive-shared-body", next.temperature_reason)
        assertEquals(95, next.last_reliable_temperature)
        assertEquals(60_000L, next.last_reliable_temperature_at_unix_ms)
        assertEquals("95°", presentation.temperatureText)
        assertEquals("just now", presentation.staleText)
        assertNull(presentation.detailText)
    }

    @Test
    fun `optimistic setpoint change clears previous eta`() {
        val body = BodyState(
            on = true,
            active = true,
            temperature = 95,
            temperature_reliable = true,
            last_reliable_temperature = 95,
            last_reliable_temperature_at_unix_ms = 10_000L,
            setpoint = 102,
            heat_mode = "heater",
            heating = "heater",
            heat_estimate = HeatEstimate(
                available = true,
                minutes_remaining = 13,
                current_temperature = 95,
                target_temperature = 102,
                confidence = "medium",
                source = "learned",
                reason = "estimating",
                observed_rate_per_hour = null,
                learned_rate_per_hour = 8.0,
                configured_rate_per_hour = 9.0,
                baseline_rate_per_hour = 8.5,
                updated_at_unix_ms = 10_000L,
            ),
            heat_estimate_display = HeatEstimateDisplay(
                state = "ready",
                reason = null,
                available_in_seconds = null,
                minutes_remaining = 13,
                target_temperature = 102,
            ),
        )

        val next = body.optimisticSetpointChange(104)
        val presentation = next.temperaturePresentation(nowMs = 40_000L)

        assertEquals(104, next.setpoint)
        assertNull(next.heat_estimate)
        assertEquals("95°", presentation.temperatureText)
        assertEquals("just now", presentation.staleText)
        assertNull(presentation.detailText)
    }
}
