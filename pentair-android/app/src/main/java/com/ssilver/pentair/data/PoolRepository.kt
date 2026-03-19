package com.ssilver.pentair.data

import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import com.ssilver.pentair.discovery.DaemonDiscovery
import com.squareup.moshi.Moshi
import com.squareup.moshi.kotlin.reflect.KotlinJsonAdapterFactory
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import retrofit2.Retrofit
import retrofit2.converter.moshi.MoshiConverterFactory
import javax.inject.Inject
import javax.inject.Singleton

enum class ConnectionState { DISCOVERING, CONNECTED, DISCONNECTED }

data class PendingChange(
    val description: String,
    val verify: (PoolSystem) -> Boolean,
    val appliedAt: Long = System.currentTimeMillis(),
)

@Singleton
class PoolRepository @Inject constructor(
    private val okHttp: OkHttpClient,
    private val discovery: DaemonDiscovery,
) : DefaultLifecycleObserver {

    private val _state = MutableStateFlow<PoolSystem?>(null)
    val state: StateFlow<PoolSystem?> = _state.asStateFlow()

    private val _connectionState = MutableStateFlow(ConnectionState.DISCOVERING)
    val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    private val _pendingChanges = mutableListOf<PendingChange>()
    private val _rejections = MutableSharedFlow<String>(extraBufferCapacity = 5)
    val rejections: SharedFlow<String> = _rejections.asSharedFlow()

    private var api: PoolApiClient? = null
    private var webSocket: WebSocket? = null
    private var baseUrl: String? = null
    private var reconnectDelay = 1000L
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val stateLock = Any()

    private fun applyOptimistic(
        description: String,
        mutate: (PoolSystem) -> PoolSystem,
        verify: (PoolSystem) -> Boolean,
    ) {
        synchronized(stateLock) {
            val current = _state.value ?: return
            _state.value = mutate(current)
            _pendingChanges.add(PendingChange(description, verify))
        }
    }

    suspend fun connect() {
        _connectionState.value = ConnectionState.DISCOVERING
        val addr = discovery.discover() ?: discovery.cachedAddress()
        android.util.Log.d("PoolRepo", "Discovery result: $addr")
        if (addr == null) {
            android.util.Log.e("PoolRepo", "No daemon found — discovery returned null")
            _connectionState.value = ConnectionState.DISCONNECTED
            return
        }
        android.util.Log.d("PoolRepo", "Connecting to daemon at $addr")
        baseUrl = addr
        val moshi = Moshi.Builder().add(KotlinJsonAdapterFactory()).build()
        api = Retrofit.Builder()
            .baseUrl(addr)
            .client(okHttp)
            .addConverterFactory(MoshiConverterFactory.create(moshi))
            .build()
            .create(PoolApiClient::class.java)

        refresh()
        connectWebSocket()
    }

    suspend fun refresh() {
        try {
            android.util.Log.d("PoolRepo", "Refreshing from ${baseUrl}/api/pool")
            val result = api?.getPool()
            android.util.Log.d("PoolRepo", "Got pool data: pool=${result?.pool?.temperature}°F spa=${result?.spa?.temperature}°F")
            synchronized(stateLock) {
                _state.value = result
            }
            _connectionState.value = ConnectionState.CONNECTED
            reconnectDelay = 1000L

            if (result != null) {
                reconcilePendingChanges(result)
            }
        } catch (e: Exception) {
            android.util.Log.e("PoolRepo", "Refresh failed: ${e.message}", e)
            _connectionState.value = ConnectionState.DISCONNECTED
        }
    }

    private fun reconcilePendingChanges(serverState: PoolSystem) {
        val now = System.currentTimeMillis()
        synchronized(stateLock) {
            val iterator = _pendingChanges.iterator()
            while (iterator.hasNext()) {
                val change = iterator.next()
                if (change.verify(serverState)) {
                    // Server confirmed the change — remove silently
                    android.util.Log.d("PoolRepo", "Confirmed: ${change.description}")
                    iterator.remove()
                } else if (now - change.appliedAt > 5000) {
                    // Grace period elapsed and server doesn't reflect the change — reject
                    android.util.Log.w("PoolRepo", "Rejected: ${change.description}")
                    _rejections.tryEmit("${change.description} didn't take effect")
                    iterator.remove()
                }
                // else: still within grace period, leave it pending
            }
        }
    }

    private fun connectWebSocket() {
        val wsUrl = (baseUrl ?: return).replace("http://", "ws://") + "/api/ws"
        val request = Request.Builder().url(wsUrl).build()
        webSocket = okHttp.newWebSocket(request, object : WebSocketListener() {
            override fun onMessage(ws: WebSocket, text: String) {
                scope.launch { refresh() }
            }
            override fun onFailure(ws: WebSocket, t: Throwable, response: Response?) {
                _connectionState.value = ConnectionState.DISCONNECTED
                scope.launch {
                    delay(reconnectDelay)
                    reconnectDelay = (reconnectDelay * 2).coerceAtMost(30000L)
                    connectWebSocket()
                }
            }
            override fun onClosed(ws: WebSocket, code: Int, reason: String) {
                _connectionState.value = ConnectionState.DISCONNECTED
            }
        })
    }

    // Lifecycle: disconnect WS on background, reconnect on foreground
    override fun onStart(owner: LifecycleOwner) {
        scope.launch {
            if (_connectionState.value == ConnectionState.DISCONNECTED) {
                connect()
            } else {
                webSocket?.close(1000, "reconnecting")
                webSocket = null
                refresh()
                connectWebSocket()
            }
        }
    }

    override fun onStop(owner: LifecycleOwner) {
        webSocket?.close(1000, "backgrounded")
        webSocket = null
    }

    // Action methods
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
                "off" -> api?.spaOff()
                "spa" -> {
                    if (preMutationSpa?.accessories?.get("jets") == true) api?.jetsOff()
                    if (preMutationSpa?.on != true) api?.spaOn()
                }
                "jets" -> {
                    if (preMutationSpa?.on != true) api?.spaOn()
                    api?.jetsOn()
                }
            }
        } catch (e: Exception) {
            _rejections.tryEmit("Spa ${state} failed: ${e.message}")
            try { refresh() } catch (_: Exception) {}
            return
        }
        delay(1500)
        refresh()
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
                api?.lightsOff()
            } else {
                api?.lightsMode(mapOf("mode" to mode))
            }
        } catch (e: Exception) {
            _rejections.tryEmit("Light mode ${mode} failed: ${e.message}")
            try { refresh() } catch (_: Exception) {}
            return
        }
        delay(500)
        refresh()
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
                "pool" -> api?.poolHeat(mapOf("setpoint" to temp))
                "spa" -> api?.spaHeat(mapOf("setpoint" to temp))
            }
        } catch (e: Exception) {
            _rejections.tryEmit("${body} setpoint ${temp} failed: ${e.message}")
            try { refresh() } catch (_: Exception) {}
            return
        }
        delay(2000)
        refresh()
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
            if (on) api?.auxOn(id) else api?.auxOff(id)
        } catch (e: Exception) {
            _rejections.tryEmit("${id} ${if (on) "on" else "off"} failed: ${e.message}")
            try { refresh() } catch (_: Exception) {}
            return
        }
        delay(1000)
        refresh()
    }
}
