package com.ssilver.pentair.ui

import com.ssilver.pentair.data.BodyState
import com.ssilver.pentair.data.SpaState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class HeatingStatusUiTest {
    @Test
    fun `pool status is blank while spa owns shared system`() {
        val pool = BodyState(
            on = false,
            active = false,
            temperature = 78,
            setpoint = 82,
            heat_mode = "heater",
            heating = "off",
        )
        val spa = SpaState(
            on = true,
            active = true,
            temperature = 101,
            setpoint = 104,
            heat_mode = "heater",
            heating = "heating",
            accessories = mapOf("jets" to true),
        )

        assertNull(poolHeatingStatus(pool, spa, sharedPump = true))
    }

    @Test
    fun `spa status is blank while pool owns shared system`() {
        val pool = BodyState(
            on = true,
            active = true,
            temperature = 81,
            setpoint = 84,
            heat_mode = "heater",
            heating = "off",
        )
        val spa = SpaState(
            on = false,
            active = false,
            temperature = 99,
            setpoint = 102,
            heat_mode = "heater",
            heating = "off",
            accessories = emptyMap(),
        )

        assertNull(spaHeatingStatus(spa, pool, sharedPump = true))
    }

    @Test
    fun `off body still shows off when shared pump is not in play`() {
        val pool = BodyState(
            on = false,
            active = false,
            temperature = 78,
            setpoint = 82,
            heat_mode = "heater",
            heating = "off",
        )

        val status = poolHeatingStatus(pool, spa = null, sharedPump = false)

        assertEquals("Off", status?.label)
    }

    @Test
    fun `sensor warmup body shows heating instead of heat error`() {
        val spa = SpaState(
            on = true,
            active = true,
            temperature = 93,
            temperature_reliable = false,
            temperature_reason = "sensor-warmup",
            last_reliable_temperature = 93,
            last_reliable_temperature_at_unix_ms = 1_000L,
            setpoint = 102,
            heat_mode = "heat-pump",
            heating = "off",
            accessories = emptyMap(),
        )

        val status = spaHeatingStatus(spa, pool = null, sharedPump = true)

        assertEquals("Heating", status?.label)
    }
}
