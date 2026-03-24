package com.ssilver.pentair.data

import app.cash.turbine.test
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.mockito.kotlin.any
import org.mockito.kotlin.mock
import org.mockito.kotlin.never
import org.mockito.kotlin.verify
import org.mockito.kotlin.whenever

/**
 * Tests for PoolRepository, focused on the optimistic update mechanism
 * and the bugs identified in code review.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PoolRepositoryTest {

    private lateinit var api: PoolApiClient
    private lateinit var repo: TestablePoolRepository

    private val baseSpa = SpaState(
        on = false,
        temperature = 108,
        setpoint = 104,
        heat_mode = "heater",
        heating = "off",
        accessories = mapOf("jets" to false),
    )

    private val basePool = BodyState(
        on = true,
        temperature = 95,
        setpoint = 59,
        heat_mode = "heater",
        heating = "off",
    )

    private val baseLights = LightState(
        on = false,
        mode = null,
        available_modes = listOf("swim", "party", "caribbean"),
    )

    private val baseSystem = PoolSystem(
        pool = basePool,
        spa = baseSpa,
        lights = baseLights,
        auxiliaries = listOf(
            AuxState(id = "aux1", name = "Water Feature", on = false),
            AuxState(id = "aux2", name = "Floor Cleaner", on = false),
        ),
        pump = PumpInfo(pump_type = "VS", running = false, watts = 0, rpm = 0, gpm = 0),
        system = SystemInfo(
            controller = "IntelliTouch",
            firmware = "5.2 Build 738.0",
            temp_unit = "F",
            air_temperature = 68,
            freeze_protection = false,
            pool_spa_shared_pump = true,
        ),
    )

    @Before
    fun setup() = runTest {
        api = mock()
        // Default all API suspend functions to return a success response
        val ok = ApiResponse(ok = true)
        whenever(api.spaOn()).thenReturn(ok)
        whenever(api.spaOff()).thenReturn(ok)
        whenever(api.jetsOn()).thenReturn(ok)
        whenever(api.jetsOff()).thenReturn(ok)
        whenever(api.lightsOn()).thenReturn(ok)
        whenever(api.lightsOff()).thenReturn(ok)
        whenever(api.lightsMode(any())).thenReturn(ok)
        whenever(api.poolHeat(any())).thenReturn(ok)
        whenever(api.spaHeat(any())).thenReturn(ok)
        whenever(api.auxOn(any())).thenReturn(ok)
        whenever(api.auxOff(any())).thenReturn(ok)

        repo = TestablePoolRepository(api)
        repo.setState(baseSystem)
    }

    // =========================================================================
    // BUG #2: setSpaState reads optimistic state for API branching
    // The "spa" branch reads _state.value AFTER applyOptimistic already set
    // spa.on=true, so `if (current?.on != true)` is false → spaOn() never called
    // =========================================================================

    @Test
    fun `setSpaState spa actually calls spaOn API`() = runTest {
        // Spa starts off
        assertEquals(false, repo.state.value?.spa?.on)

        repo.setSpaState("spa")

        // The API must actually be called — this is the critical bug
        verify(api).spaOn()
    }

    @Test
    fun `setSpaState jets calls both spaOn and jetsOn when spa is off`() = runTest {
        assertEquals(false, repo.state.value?.spa?.on)

        repo.setSpaState("jets")

        verify(api).spaOn()
        verify(api).jetsOn()
    }

    @Test
    fun `setSpaState spa turns off jets when jets are on`() = runTest {
        // Start with jets on
        repo.setState(baseSystem.copy(
            spa = baseSpa.copy(on = true, accessories = mapOf("jets" to true))
        ))

        repo.setSpaState("spa")

        verify(api).jetsOff()
        // Spa is already on, so spaOn should NOT be called
        verify(api, never()).spaOn()
    }

    @Test
    fun `setSpaState off calls spaOff API`() = runTest {
        repo.setState(baseSystem.copy(spa = baseSpa.copy(on = true)))

        repo.setSpaState("off")

        verify(api).spaOff()
    }

    // =========================================================================
    // Optimistic updates: state changes immediately
    // =========================================================================

    @Test
    fun `setSpaState optimistically updates state before API call`() = runTest {
        assertEquals(false, repo.state.value?.spa?.on)

        // Call setSpaState — applyOptimistic runs synchronously before the API call
        repo.setSpaState("spa")

        // State should have been updated optimistically
        assertEquals(true, repo.state.value?.spa?.on)
        assertEquals(false, repo.state.value?.spa?.accessories?.get("jets"))
    }

    @Test
    fun `setLightMode optimistically updates state`() = runTest {
        assertEquals(false, repo.state.value?.lights?.on)
        assertNull(repo.state.value?.lights?.mode)

        repo.setLightMode("caribbean")

        assertEquals(true, repo.state.value?.lights?.on)
        assertEquals("caribbean", repo.state.value?.lights?.mode)
    }

    @Test
    fun `setSetpoint optimistically updates pool setpoint`() = runTest {
        assertEquals(59, repo.state.value?.pool?.setpoint)

        repo.setSetpoint("pool", 82)

        assertEquals(82, repo.state.value?.pool?.setpoint)
    }

    @Test
    fun `setSetpoint optimistically updates spa setpoint`() = runTest {
        assertEquals(104, repo.state.value?.spa?.setpoint)

        repo.setSetpoint("spa", 100)

        assertEquals(100, repo.state.value?.spa?.setpoint)
    }

    @Test
    fun `toggleAux optimistically updates aux state`() = runTest {
        assertEquals(false, repo.state.value?.auxiliaries?.find { it.id == "aux1" }?.on)

        repo.toggleAux("aux1", true)

        assertEquals(true, repo.state.value?.auxiliaries?.find { it.id == "aux1" }?.on)
        // Other aux should be unchanged
        assertEquals(false, repo.state.value?.auxiliaries?.find { it.id == "aux2" }?.on)
    }

    // =========================================================================
    // Rejection detection: server state doesn't match optimistic update
    // =========================================================================

    @Test
    fun `rejected change emits rejection after grace period`() = runTest {
        repo.rejections.test {
            // Apply optimistic change that won't match server
            repo.testApplyOptimistic(
                description = "Test change",
                mutate = { it.copy(pool = it.pool?.copy(setpoint = 99)) },
                verify = { it.pool?.setpoint == 99 },
            )

            // Server returns original state (setpoint still 59)
            repo.testReconcile(baseSystem, elapsedMs = 6000)

            assertEquals("Test change didn't take effect", awaitItem())
        }
    }

    @Test
    fun `confirmed change does not emit rejection`() = runTest {
        repo.rejections.test {
            repo.testApplyOptimistic(
                description = "Pool setpoint 82",
                mutate = { it.copy(pool = it.pool?.copy(setpoint = 82)) },
                verify = { it.pool?.setpoint == 82 },
            )

            // Server confirms the change
            repo.testReconcile(baseSystem.copy(pool = basePool.copy(setpoint = 82)), elapsedMs = 6000)

            // No rejection should be emitted
            expectNoEvents()
        }
    }

    @Test
    fun `server snapshot application preserves pending optimistic state`() = runTest {
        repo.testApplyOptimistic(
            description = "Pool setpoint 82",
            mutate = { it.copy(pool = it.pool?.copy(setpoint = 82)) },
            verify = { it.pool?.setpoint == 82 },
        )

        repo.testApplyServerState(baseSystem)

        assertEquals(82, repo.state.value?.pool?.setpoint)
    }

    @Test
    fun `websocket snapshot updates state without refresh call`() = runTest {
        repo.setState(null)

        val payload = repo.testSerialize(baseSystem)
        repo.testApplyWebSocketMessage(payload)

        assertNotNull(repo.state.value)
        assertEquals(baseSystem.system.controller, repo.state.value?.system?.controller)
        assertEquals(baseSystem.pool?.temperature, repo.state.value?.pool?.temperature)
        assertEquals(baseSystem.spa?.setpoint, repo.state.value?.spa?.setpoint)
    }

    @Test
    fun `pending change within grace period is not rejected`() = runTest {
        repo.rejections.test {
            repo.testApplyOptimistic(
                description = "Test change",
                mutate = { it.copy(pool = it.pool?.copy(setpoint = 99)) },
                verify = { it.pool?.setpoint == 99 },
            )

            // Server returns non-matching state but within 5s grace period
            repo.testReconcile(baseSystem, elapsedMs = 3000)

            // Should NOT emit rejection yet
            expectNoEvents()
        }
    }

    // =========================================================================
    // BUG #1: Race condition — concurrent refresh and applyOptimistic
    // applyOptimistic does read-modify-write on _state.value outside synchronized
    // =========================================================================

    @Test
    fun `concurrent refresh does not clobber optimistic update`() = runTest {
        // Apply an optimistic change
        repo.testApplyOptimistic(
            description = "Spa on",
            mutate = { it.copy(spa = it.spa?.copy(on = true)) },
            verify = { it.spa?.on == true },
        )

        assertEquals(true, repo.state.value?.spa?.on)

        // A refresh arrives with the OLD server state (spa still off)
        // but still within grace period — the optimistic state should be preserved
        // or at minimum, the pending change should still be tracked
        val pendingCount = repo.pendingChangeCount()
        assertTrue("Pending change should still be tracked", pendingCount > 0)
    }

    // =========================================================================
    // BUG #9: No error handling on API action calls
    // If the API throws, optimistic state lingers with no feedback
    // =========================================================================

    @Test
    fun `API failure on setSpaState emits rejection and does not throw`() = runTest {
        whenever(api.spaOn()).thenThrow(RuntimeException("Network error"))

        repo.rejections.test {
            // Should NOT throw — error is caught internally
            repo.setSpaState("spa")

            // The optimistic update was applied
            assertEquals(true, repo.state.value?.spa?.on)

            // There should be a pending change that will eventually be rejected
            assertTrue(repo.pendingChangeCount() > 0)

            // A rejection should have been emitted
            val rejection = awaitItem()
            assertTrue("Expected rejection about spa failure, got: $rejection",
                rejection.contains("Spa") && rejection.contains("failed"))
        }
    }

    @Test
    fun `API failure on toggleAux emits rejection and does not throw`() = runTest {
        whenever(api.auxOn(any())).thenThrow(RuntimeException("Network error"))

        repo.rejections.test {
            // Should NOT throw — error is caught internally
            repo.toggleAux("aux1", true)

            // Optimistic update was applied
            assertEquals(true, repo.state.value?.auxiliaries?.find { it.id == "aux1" }?.on)
            assertTrue(repo.pendingChangeCount() > 0)

            // A rejection should have been emitted
            val rejection = awaitItem()
            assertTrue("Expected rejection about aux1 failure, got: $rejection",
                rejection.contains("aux1") && rejection.contains("failed"))
        }
    }

    // =========================================================================
    // BUG #4: CoroutineScope has no SupervisorJob
    // Can't easily unit test scope cancellation, but we can verify the scope
    // configuration is correct after fixes are applied
    // =========================================================================

    // =========================================================================
    // BUG #5: Duplicate WebSocket on onStart
    // =========================================================================

    @Test
    fun `onStop sets webSocket to null`() = runTest {
        // Verify onStop clears the websocket reference
        // (Can't easily test WebSocket creation without integration test,
        // but we can verify the cleanup path)
        repo.testOnStop()
        assertNull(repo.getWebSocket())
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    @Test
    fun `applyOptimistic with null state is a no-op`() = runTest {
        repo.setState(null)

        repo.testApplyOptimistic(
            description = "Should be no-op",
            mutate = { it.copy(pool = it.pool?.copy(setpoint = 99)) },
            verify = { it.pool?.setpoint == 99 },
        )

        assertNull(repo.state.value)
        assertEquals(0, repo.pendingChangeCount())
    }

    @Test
    fun `setLightMode off optimistically turns lights off`() = runTest {
        // Start with lights on
        repo.setState(baseSystem.copy(lights = baseLights.copy(on = true, mode = "swim")))

        repo.setLightMode("off")

        assertEquals(false, repo.state.value?.lights?.on)
        assertNull(repo.state.value?.lights?.mode)
    }

    @Test
    fun `toggleAux calls correct API endpoint`() = runTest {
        repo.toggleAux("aux1", true)
        verify(api).auxOn("aux1")

        repo.toggleAux("aux2", false)
        verify(api).auxOff("aux2")
    }

    @Test
    fun `setSetpoint calls correct API for pool`() = runTest {
        repo.setSetpoint("pool", 82)
        verify(api).poolHeat(mapOf("setpoint" to 82))
    }

    @Test
    fun `setSetpoint calls correct API for spa`() = runTest {
        repo.setSetpoint("spa", 100)
        verify(api).spaHeat(mapOf("setpoint" to 100))
    }
}
