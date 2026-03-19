package com.ssilver.pentair.data

import com.squareup.moshi.Moshi
import com.squareup.moshi.kotlin.reflect.KotlinJsonAdapterFactory
import kotlinx.coroutines.runBlocking
import okhttp3.OkHttpClient
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.FixMethodOrder
import org.junit.Test
import org.junit.runners.MethodSorters
import retrofit2.Retrofit
import retrofit2.converter.moshi.MoshiConverterFactory
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue

/**
 * Live integration tests against the real daemon API.
 * These tests talk to actual pool hardware via the daemon.
 *
 * Run with: ./gradlew testDebugUnitTest --tests "*.LiveApiTest" -Dpentair.live=true
 *
 * Each test saves state before, acts, verifies, then restores.
 * Tests are ordered to avoid conflicting hardware state.
 */
@FixMethodOrder(MethodSorters.NAME_ASCENDING)
class LiveApiTest {

    private lateinit var api: PoolApiClient

    companion object {
        // Set via -Dpentair.daemon=http://host:port or defaults to localhost
        private val DAEMON_URL = System.getProperty("pentair.daemon", "http://localhost:8080")
        private val LIVE_ENABLED = System.getProperty("pentair.live", "false").toBoolean()
    }

    @Before
    fun setup() {
        assumeTrue("Live tests disabled — pass -Dpentair.live=true", LIVE_ENABLED)

        val okHttp = OkHttpClient.Builder()
            .connectTimeout(5, TimeUnit.SECONDS)
            .readTimeout(10, TimeUnit.SECONDS)
            .build()

        val moshi = Moshi.Builder().add(KotlinJsonAdapterFactory()).build()

        api = Retrofit.Builder()
            .baseUrl(DAEMON_URL)
            .client(okHttp)
            .addConverterFactory(MoshiConverterFactory.create(moshi))
            .build()
            .create(PoolApiClient::class.java)
    }

    private suspend fun getState(): PoolSystem {
        val state = api.getPool()
        assertNotNull("Daemon returned null state", state)
        return state
    }

    private fun waitForHardware(ms: Long = 2000) = Thread.sleep(ms)

    // =========================================================================
    // Test 1: GET /api/pool returns valid state
    // =========================================================================

    @Test
    fun `01 getPool returns valid system state`() = runBlocking {
        val state = getState()

        assertNotNull("pool should not be null", state.pool)
        assertNotNull("spa should not be null", state.spa)
        assertNotNull("lights should not be null", state.lights)
        assertNotNull("system should not be null", state.system)
        assertTrue("pool temp should be > 0", state.pool!!.temperature > 0)
        assertTrue("spa temp should be > 0", state.spa!!.temperature > 0)
        assertTrue("air temp should be > 0", state.system.air_temperature > 0)
        assertTrue("should have auxiliaries", state.auxiliaries.isNotEmpty())
        assertEquals("IntelliTouch", state.system.controller)
    }

    // =========================================================================
    // Test 2: Spa on/off cycle
    // =========================================================================

    @Test
    fun `02 spa on then off restores state`() = runBlocking {
        val before = getState()
        val spaWasOn = before.spa!!.on

        // Turn spa on
        val onResult = api.spaOn()
        assertTrue("spaOn should succeed", onResult.ok)
        waitForHardware()

        val afterOn = getState()
        assertTrue("spa should be on after spaOn", afterOn.spa!!.on)

        // Turn spa off
        val offResult = api.spaOff()
        assertTrue("spaOff should succeed", offResult.ok)
        waitForHardware()

        val afterOff = getState()
        assertEquals("spa should be off after spaOff", false, afterOff.spa!!.on)

        // Restore original state if spa was on
        if (spaWasOn) {
            api.spaOn()
            waitForHardware()
        }
    }

    // =========================================================================
    // Test 3: Jets on/off (requires spa to be on first)
    // =========================================================================

    @Test
    fun `03 jets on then off restores state`() = runBlocking {
        val before = getState()
        val spaWasOn = before.spa!!.on
        val jetsWereOn = before.spa.accessories["jets"] == true

        // Ensure spa is on first
        if (!spaWasOn) {
            api.spaOn()
            waitForHardware()
        }

        // Turn jets on
        val jetsOnResult = api.jetsOn()
        assertTrue("jetsOn should succeed", jetsOnResult.ok)
        waitForHardware()

        val afterJets = getState()
        assertTrue("jets should be on", afterJets.spa!!.accessories["jets"] == true)
        assertTrue("spa should still be on with jets", afterJets.spa.on)

        // Turn jets off
        val jetsOffResult = api.jetsOff()
        assertTrue("jetsOff should succeed", jetsOffResult.ok)
        waitForHardware()

        val afterJetsOff = getState()
        assertTrue("jets should be off", afterJetsOff.spa!!.accessories["jets"] != true)

        // Restore
        if (!spaWasOn) {
            api.spaOff()
            waitForHardware()
        } else if (jetsWereOn) {
            api.jetsOn()
            waitForHardware()
        }
    }

    // =========================================================================
    // Test 4: Spa setpoint change and restore
    // =========================================================================

    @Test
    fun `04 spa setpoint change and restore`() = runBlocking {
        val before = getState()
        val originalSetpoint = before.spa!!.setpoint

        // Change setpoint by 1 degree
        val newSetpoint = if (originalSetpoint < 104) originalSetpoint + 1 else originalSetpoint - 1
        val result = api.spaHeat(mapOf("setpoint" to newSetpoint))
        assertTrue("spaHeat should succeed", result.ok)
        waitForHardware(3000)

        val after = getState()
        assertEquals("spa setpoint should be updated", newSetpoint, after.spa!!.setpoint)

        // Restore
        api.spaHeat(mapOf("setpoint" to originalSetpoint))
        waitForHardware(3000)

        val restored = getState()
        assertEquals("spa setpoint should be restored", originalSetpoint, restored.spa!!.setpoint)
    }

    // =========================================================================
    // Test 5: Pool setpoint change and restore
    // =========================================================================

    @Test
    fun `05 pool setpoint change and restore`() = runBlocking {
        val before = getState()
        val originalSetpoint = before.pool!!.setpoint

        val newSetpoint = if (originalSetpoint < 90) originalSetpoint + 1 else originalSetpoint - 1
        val result = api.poolHeat(mapOf("setpoint" to newSetpoint))
        assertTrue("poolHeat should succeed", result.ok)
        waitForHardware(3000)

        val after = getState()
        assertEquals("pool setpoint should be updated", newSetpoint, after.pool!!.setpoint)

        // Restore
        api.poolHeat(mapOf("setpoint" to originalSetpoint))
        waitForHardware(3000)

        val restored = getState()
        assertEquals("pool setpoint should be restored", originalSetpoint, restored.pool!!.setpoint)
    }

    // =========================================================================
    // Test 6: Light mode change and restore
    // =========================================================================

    @Test
    fun `06 light mode set and off`() = runBlocking {
        val before = getState()
        val lightsWereOn = before.lights!!.on
        val originalMode = before.lights.mode

        // Set a light mode
        val result = api.lightsMode(mapOf("mode" to "caribbean"))
        assertTrue("lightsMode should succeed", result.ok)
        waitForHardware()

        // Turn lights off
        val offResult = api.lightsOff()
        assertTrue("lightsOff should succeed", offResult.ok)
        waitForHardware()

        // Restore if lights were on
        if (lightsWereOn && originalMode != null) {
            api.lightsMode(mapOf("mode" to originalMode))
            waitForHardware()
        }
    }

    // =========================================================================
    // Test 7: Auxiliary toggle and restore
    // =========================================================================

    @Test
    fun `07 auxiliary toggle and restore`() = runBlocking {
        val before = getState()
        // Find first aux that's off to toggle on, or first that's on to toggle off
        val aux = before.auxiliaries.firstOrNull() ?: return@runBlocking
        val wasOn = aux.on

        // Toggle
        if (wasOn) {
            val result = api.auxOff(aux.id)
            assertTrue("auxOff should succeed", result.ok)
        } else {
            val result = api.auxOn(aux.id)
            assertTrue("auxOn should succeed", result.ok)
        }
        waitForHardware()

        val after = getState()
        val afterAux = after.auxiliaries.find { it.id == aux.id }
        assertNotNull("aux should still exist", afterAux)
        assertEquals("aux state should have toggled", !wasOn, afterAux!!.on)

        // Restore
        if (wasOn) {
            api.auxOn(aux.id)
        } else {
            api.auxOff(aux.id)
        }
        waitForHardware()

        val restored = getState()
        val restoredAux = restored.auxiliaries.find { it.id == aux.id }
        assertEquals("aux should be restored", wasOn, restoredAux!!.on)
    }

    // =========================================================================
    // Test 8: Rapid spa toggle (the scenario that triggers Bug #2)
    // =========================================================================

    @Test
    fun `08 rapid spa on then off completes without error`() = runBlocking {
        val before = getState()
        val spaWasOn = before.spa!!.on

        // Turn on
        api.spaOn()
        waitForHardware(500) // Short delay — rapid toggle

        // Turn off immediately
        api.spaOff()
        waitForHardware()

        val after = getState()
        assertEquals("spa should be off after rapid toggle", false, after.spa!!.on)

        // Restore if needed
        if (spaWasOn) {
            api.spaOn()
            waitForHardware()
        }
    }
}
