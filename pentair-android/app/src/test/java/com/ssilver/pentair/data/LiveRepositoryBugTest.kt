package com.ssilver.pentair.data

import com.squareup.moshi.Moshi
import com.squareup.moshi.kotlin.reflect.KotlinJsonAdapterFactory
import kotlinx.coroutines.runBlocking
import okhttp3.OkHttpClient
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.FixMethodOrder
import org.junit.Test
import org.junit.runners.MethodSorters
import retrofit2.Retrofit
import retrofit2.converter.moshi.MoshiConverterFactory
import java.util.concurrent.TimeUnit

/**
 * Live tests that go through the REPOSITORY layer (including the buggy
 * optimistic update code) and then check HARDWARE state via a direct
 * API call to verify the command actually reached the adapter.
 *
 * These tests prove Bug #2: setSpaState reads optimistic state for API
 * branching, so the API call is silently skipped while the UI shows success.
 *
 * Run with: ./gradlew testDebugUnitTest --tests "*.LiveRepositoryBugTest" -Dpentair.live=true
 */
@FixMethodOrder(MethodSorters.NAME_ASCENDING)
class LiveRepositoryBugTest {

    private lateinit var api: PoolApiClient
    private lateinit var repo: TestablePoolRepository

    companion object {
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

        // Wire the repository to the REAL API
        repo = TestablePoolRepository(api)
    }

    private suspend fun getHardwareState(): PoolSystem = api.getPool()

    private fun waitForHardware(ms: Long = 2500) = Thread.sleep(ms)

    // =========================================================================
    // BUG #2 PROOF: setSpaState("spa") through the repository
    //
    // Expected: spa turns on in hardware
    // Actual (bug): optimistic update shows spa on, but spaOn() API is never
    //               called because the code reads _state.value AFTER the
    //               optimistic mutation set spa.on=true, making the condition
    //               `if (current?.on != true)` evaluate to false.
    // =========================================================================

    @Test
    fun `01 BUG2 setSpaState spa through repository - hardware should turn on`() = runBlocking {
        val before = getHardwareState()
        val spaWasOn = before.spa!!.on

        // Ensure spa starts OFF so we can test turning it ON
        if (spaWasOn) {
            api.spaOff()
            waitForHardware()
        }

        // Seed the repository with current hardware state
        val freshState = getHardwareState()
        repo.setState(freshState)
        assertEquals("spa should be off before test", false, repo.state.value?.spa?.on)

        // Call through the repository — this is the buggy path
        repo.setSpaState("spa")

        // Repository's optimistic state says spa is on
        assertEquals("optimistic state shows spa on", true, repo.state.value?.spa?.on)

        // But did the hardware ACTUALLY turn on?
        waitForHardware()
        val hardwareState = getHardwareState()

        // THIS IS THE BUG: hardware spa is still OFF because spaOn() was never called
        // After fixing, this assertion should pass
        assertEquals(
            "BUG #2: hardware spa should be on, but spaOn() was never called due to reading optimistic state",
            true,
            hardwareState.spa!!.on,
        )

        // Restore
        api.spaOff()
        waitForHardware()
    }

    // =========================================================================
    // BUG #2 PROOF: setSpaState("jets") through the repository when spa is off
    //
    // Expected: spa turns on AND jets turn on
    // Actual (bug): optimistic update shows jets on, but spaOn() is skipped
    //               because optimistic state already has spa.on=true
    // =========================================================================

    @Test
    fun `02 BUG2 setSpaState jets through repository - hardware should have jets`() = runBlocking {
        val before = getHardwareState()
        val spaWasOn = before.spa!!.on

        // Ensure spa starts OFF
        if (spaWasOn) {
            api.spaOff()
            waitForHardware()
        }

        val freshState = getHardwareState()
        repo.setState(freshState)
        assertEquals("spa should be off", false, repo.state.value?.spa?.on)

        // Call through the buggy repository
        repo.setSpaState("jets")

        // Optimistic state shows jets on
        assertEquals("optimistic: spa on", true, repo.state.value?.spa?.on)
        assertEquals("optimistic: jets on", true, repo.state.value?.spa?.accessories?.get("jets"))

        // Check hardware
        waitForHardware()
        val hardwareState = getHardwareState()

        // The bug: spaOn() was skipped, so spa might not be on, and jets might not work
        assertEquals(
            "BUG #2: hardware spa should be on for jets to work",
            true,
            hardwareState.spa!!.on,
        )
        assertEquals(
            "BUG #2: hardware jets should be on",
            true,
            hardwareState.spa.accessories["jets"],
        )

        // Restore
        api.jetsOff()
        waitForHardware(500)
        api.spaOff()
        waitForHardware()
    }

    // =========================================================================
    // BUG #2 PROOF: setSpaState("spa") when jets are on should turn jets OFF
    //
    // Expected: jets turn off, spa stays on
    // Actual (bug): jetsOff() is skipped because optimistic state already
    //               cleared jets in the accessories map
    // =========================================================================

    @Test
    fun `03 BUG2 setSpaState spa with jets on - hardware should turn jets off`() = runBlocking {
        // Set up: spa on with jets on
        api.spaOn()
        waitForHardware()
        api.jetsOn()
        waitForHardware()

        val freshState = getHardwareState()
        assertTrue("setup: spa should be on", freshState.spa!!.on)
        assertEquals("setup: jets should be on", true, freshState.spa.accessories["jets"])

        repo.setState(freshState)

        // Call through the buggy repository — switch from "jets" to "spa"
        repo.setSpaState("spa")

        // Optimistic state shows jets off
        assertEquals("optimistic: jets off", false, repo.state.value?.spa?.accessories?.get("jets"))

        // Check hardware
        waitForHardware()
        val hardwareState = getHardwareState()

        // The bug: jetsOff() was skipped because optimistic state already had jets=false
        assertTrue(
            "BUG #2: hardware jets should be off after switching to spa mode",
            hardwareState.spa!!.accessories["jets"] != true,
        )

        // Restore
        api.spaOff()
        waitForHardware()
    }
}
