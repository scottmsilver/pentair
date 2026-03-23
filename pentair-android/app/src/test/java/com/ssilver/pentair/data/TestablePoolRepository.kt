package com.ssilver.pentair.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.MutableSharedFlow
import okhttp3.WebSocket

/**
 * A testable version of the repository's optimistic update logic,
 * decoupled from Android dependencies (mDNS, WebSocket, android.util.Log).
 *
 * This exercises the same applyOptimistic / reconcile / action logic
 * that the real PoolRepository uses, by extracting it into testable methods.
 */
class TestablePoolRepository(
    private val api: PoolApiClient,
) {
    private val _state = MutableStateFlow<PoolSystem?>(null)
    val state: StateFlow<PoolSystem?> = _state.asStateFlow()

    private val _pendingChanges = mutableListOf<PendingChange>()
    private val _rejections = MutableSharedFlow<String>(extraBufferCapacity = 5)
    val rejections: SharedFlow<String> = _rejections.asSharedFlow()

    private var webSocket: WebSocket? = null
    private val stateLock = Any()

    fun setState(system: PoolSystem?) {
        _state.value = system
    }

    fun pendingChangeCount(): Int = synchronized(stateLock) { _pendingChanges.size }

    fun getWebSocket(): WebSocket? = webSocket

    fun testOnStop() {
        webSocket?.close(1000, "backgrounded")
        webSocket = null
    }

    // --- Optimistic update mechanism (mirrors PoolRepository) ---

    private fun applyOptimistic(
        description: String,
        mutate: (PoolSystem) -> PoolSystem,
        verify: (PoolSystem) -> Boolean,
    ) {
        synchronized(stateLock) {
            val current = _state.value ?: return
            _state.value = mutate(current)
            _pendingChanges.add(PendingChange(description, mutate, verify))
        }
    }

    /** Exposed for tests that need to call applyOptimistic directly */
    fun testApplyOptimistic(
        description: String,
        mutate: (PoolSystem) -> PoolSystem,
        verify: (PoolSystem) -> Boolean,
    ) = applyOptimistic(description, mutate, verify)

    /** Reconcile with a given server state and simulated elapsed time */
    fun testReconcile(serverState: PoolSystem, elapsedMs: Long) {
        synchronized(stateLock) {
            val iterator = _pendingChanges.iterator()
            while (iterator.hasNext()) {
                val change = iterator.next()
                if (change.verify(serverState)) {
                    iterator.remove()
                } else if (elapsedMs > 5000) {
                    _rejections.tryEmit("${change.description} didn't take effect")
                    iterator.remove()
                }
            }
        }
    }

    // --- Action methods (mirror PoolRepository exactly) ---

    suspend fun setSpaState(state: String) {
        val preMutationSpa = _state.value?.spa  // capture BEFORE applyOptimistic

        when (state) {
            "off" -> applyOptimistic(
                description = "Spa off",
                mutate = { sys ->
                    sys.copy(spa = sys.spa?.copy(on = false, accessories = sys.spa.accessories.mapValues { false }))
                },
                verify = { sys -> sys.spa?.on == false },
            )
            "spa" -> applyOptimistic(
                description = "Spa on",
                mutate = { sys ->
                    sys.copy(spa = sys.spa?.copy(on = true, accessories = sys.spa.accessories.mapValues { false }))
                },
                verify = { sys -> sys.spa?.on == true && sys.spa.accessories["jets"] != true },
            )
            "jets" -> applyOptimistic(
                description = "Jets on",
                mutate = { sys ->
                    sys.copy(spa = sys.spa?.copy(on = true, accessories = sys.spa.accessories + ("jets" to true)))
                },
                verify = { sys -> sys.spa?.on == true && sys.spa.accessories["jets"] == true },
            )
        }

        try {
            when (state) {
                "off" -> api.spaOff()
                "spa" -> {
                    if (preMutationSpa?.accessories?.get("jets") == true) api.jetsOff()
                    if (preMutationSpa?.on != true) api.spaOn()
                }
                "jets" -> {
                    if (preMutationSpa?.on != true) api.spaOn()
                    api.jetsOn()
                }
            }
        } catch (e: Exception) {
            _rejections.tryEmit("Spa ${state} failed: ${e.message}")
            return
        }
    }

    suspend fun setLightMode(mode: String) {
        if (mode == "off") {
            applyOptimistic(
                description = "Lights off",
                mutate = { sys -> sys.copy(lights = sys.lights?.copy(on = false, mode = null)) },
                verify = { sys -> sys.lights?.on == false },
            )
        } else {
            applyOptimistic(
                description = "Light mode $mode",
                mutate = { sys -> sys.copy(lights = sys.lights?.copy(on = true, mode = mode)) },
                verify = { sys -> sys.lights?.on == true && sys.lights.mode == mode },
            )
        }

        try {
            if (mode == "off") {
                api.lightsOff()
            } else {
                api.lightsMode(mapOf("mode" to mode))
            }
        } catch (e: Exception) {
            _rejections.tryEmit("Light mode ${mode} failed: ${e.message}")
            return
        }
    }

    suspend fun setSetpoint(body: String, temp: Int) {
        when (body) {
            "pool" -> applyOptimistic(
                description = "Pool setpoint $temp",
                mutate = { sys -> sys.copy(pool = sys.pool?.copy(setpoint = temp)) },
                verify = { sys -> sys.pool?.setpoint == temp },
            )
            "spa" -> applyOptimistic(
                description = "Spa setpoint $temp",
                mutate = { sys -> sys.copy(spa = sys.spa?.copy(setpoint = temp)) },
                verify = { sys -> sys.spa?.setpoint == temp },
            )
        }

        try {
            when (body) {
                "pool" -> api.poolHeat(mapOf("setpoint" to temp))
                "spa" -> api.spaHeat(mapOf("setpoint" to temp))
            }
        } catch (e: Exception) {
            _rejections.tryEmit("${body} setpoint ${temp} failed: ${e.message}")
            return
        }
    }

    suspend fun toggleAux(id: String, on: Boolean) {
        applyOptimistic(
            description = "${id} ${if (on) "on" else "off"}",
            mutate = { sys ->
                sys.copy(auxiliaries = sys.auxiliaries.map { aux ->
                    if (aux.id == id) aux.copy(on = on) else aux
                })
            },
            verify = { sys -> sys.auxiliaries.find { it.id == id }?.on == on },
        )

        try {
            if (on) api.auxOn(id) else api.auxOff(id)
        } catch (e: Exception) {
            _rejections.tryEmit("${id} ${if (on) "on" else "off"} failed: ${e.message}")
            return
        }
    }
}
