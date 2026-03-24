package com.ssilver.pentair.data

import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import com.ssilver.pentair.discovery.DaemonDiscovery
import com.squareup.moshi.Moshi
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
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import retrofit2.Retrofit
import retrofit2.converter.moshi.MoshiConverterFactory
import java.util.concurrent.atomic.AtomicLong
import javax.inject.Inject
import javax.inject.Singleton

enum class ConnectionState { DISCOVERING, CONNECTING, CONNECTED, DISCONNECTED }

data class DiagnosticEvent(
    val timestampMillis: Long = System.currentTimeMillis(),
    val category: String,
    val message: String,
)

data class PendingChange(
    val description: String,
    val mutate: (PoolSystem) -> PoolSystem,
    val verify: (PoolSystem) -> Boolean,
    val appliedAt: Long = System.currentTimeMillis(),
)

@Singleton
class PoolRepository @Inject constructor(
    private val okHttp: OkHttpClient,
    private val discovery: DaemonDiscovery,
) : DefaultLifecycleObserver {
    private val moshi = Moshi.Builder().build()
    private val poolSystemAdapter = moshi.adapter(PoolSystem::class.java)

    private val _state = MutableStateFlow<PoolSystem?>(null)
    val state: StateFlow<PoolSystem?> = _state.asStateFlow()

    private val _connectionState = MutableStateFlow(ConnectionState.DISCOVERING)
    val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    private val _manualAddress = MutableStateFlow(discovery.cachedAddress().orEmpty())
    val manualAddress: StateFlow<String> = _manualAddress.asStateFlow()

    private val _discoveredAddress = MutableStateFlow<String?>(null)
    val discoveredAddress: StateFlow<String?> = _discoveredAddress.asStateFlow()

    private val _activeAddress = MutableStateFlow(discovery.cachedAddress())
    val activeAddress: StateFlow<String?> = _activeAddress.asStateFlow()

    private val _isTestingAddress = MutableStateFlow(false)
    val isTestingAddress: StateFlow<Boolean> = _isTestingAddress.asStateFlow()

    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    private val _diagnostics = MutableStateFlow<List<DiagnosticEvent>>(emptyList())
    val diagnostics: StateFlow<List<DiagnosticEvent>> = _diagnostics.asStateFlow()

    private val _pendingChanges = mutableListOf<PendingChange>()
    private val _rejections = MutableSharedFlow<String>(extraBufferCapacity = 5)
    val rejections: SharedFlow<String> = _rejections.asSharedFlow()

    private var api: PoolApiClient? = null
    private var webSocket: WebSocket? = null
    private var baseUrl: String? = null
    private var reconnectJob: kotlinx.coroutines.Job? = null
    private var connectJob: kotlinx.coroutines.Job? = null
    private val reconnectDelay = AtomicLong(1000L)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val stateLock = Any()

    init {
        discovery.cachedAddress()?.let { cached ->
            recordDiagnostic("startup", "Loaded cached daemon address $cached")
        }
    }

    fun setManualAddressInput(address: String) {
        _manualAddress.value = address
    }

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

    suspend fun connect() {
        while (true) {
            _connectionState.value = ConnectionState.DISCOVERING
            recordDiagnostic("discovery", "Searching for daemon")
            val discoveredAddress = discovery.discover()
            if (discoveredAddress != null) {
                _discoveredAddress.value = discoveredAddress
                if (_manualAddress.value.isBlank() || _manualAddress.value == _activeAddress.value) {
                    _manualAddress.value = discoveredAddress
                }
            }

            val candidates = discovery.connectionCandidates(discoveredAddress)
            if (candidates.isEmpty()) {
                recordDiagnostic("discovery", "No daemon found")
                _connectionState.value = ConnectionState.DISCONNECTED
                delay(reconnectDelay.get())
                reconnectDelay.getAndUpdate { (it * 2).coerceAtMost(30000L) }
                continue
            }

            if (discoveredAddress == null) {
                recordDiagnostic(
                    "discovery",
                    "NSD found nothing; probing ${candidates.joinToString()}"
                )
            }

            for (candidate in candidates) {
                if (candidate != discoveredAddress) {
                    recordDiagnostic("probe", "Trying fallback daemon address $candidate")
                }
                if (tryConnect(candidate)) return
            }

            // All fetch attempts failed — back off and retry discovery
            recordDiagnostic("http", "All connect attempts failed for ${candidates.joinToString()}")
            _connectionState.value = ConnectionState.DISCONNECTED
            delay(reconnectDelay.get())
            reconnectDelay.getAndUpdate { (it * 2).coerceAtMost(30000L) }
        }
    }

    suspend fun refresh() {
        val address = _activeAddress.value ?: return

        _isRefreshing.value = true
        try {
            ensureApi(address)
            recordDiagnostic("http", "GET $address/api/pool")
            val result = api?.getPool()
            if (result != null) {
                applyServerState(result)
            }
            _connectionState.value = ConnectionState.CONNECTED
            reconnectDelay.set(1000L)
            recordDiagnostic("http", "GET /api/pool succeeded")
        } catch (e: Exception) {
            recordDiagnostic("http", "GET /api/pool failed: ${e.message}")
            _connectionState.value = ConnectionState.DISCONNECTED
        } finally {
            _isRefreshing.value = false
        }
    }

    suspend fun applyManualAddress() {
        val address = normalizeAddress(_manualAddress.value)
        if (address == null) {
            _rejections.tryEmit("Invalid address")
            recordDiagnostic("probe", "Rejected invalid manual address '${_manualAddress.value}'")
            return
        }

        _manualAddress.value = address
        discovery.setManualAddress(address)
        recordDiagnostic("probe", "Applying manual daemon address $address")

        if (!tryConnect(address)) {
            _rejections.tryEmit("Couldn't connect to $address")
            _connectionState.value = ConnectionState.DISCONNECTED
        }
    }

    suspend fun useDiscoveredAddress() {
        val address = _discoveredAddress.value ?: return
        _manualAddress.value = address
        discovery.setManualAddress(address)
        recordDiagnostic("probe", "Using discovered daemon address $address")

        if (!tryConnect(address)) {
            _rejections.tryEmit("Couldn't connect to $address")
            _connectionState.value = ConnectionState.DISCONNECTED
        }
    }

    suspend fun testManualAddress() {
        val candidate = _manualAddress.value.ifBlank { _activeAddress.value.orEmpty() }
        val address = normalizeAddress(candidate)
        if (address == null) {
            _rejections.tryEmit("Invalid address")
            recordDiagnostic("probe", "Rejected invalid probe address '$candidate'")
            return
        }

        _isTestingAddress.value = true
        recordDiagnostic("probe", "Testing $address/api/pool")
        try {
            val result = buildApi(address).getPool()
            recordDiagnostic(
                "probe",
                "Success. Controller ${result.system.controller}, air ${result.system.air_temperature}°"
            )
            _rejections.tryEmit("Connection OK: $address")
        } catch (e: Exception) {
            recordDiagnostic("probe", "Failed: ${e.message}")
            _rejections.tryEmit(e.message ?: "Connection failed")
        } finally {
            _isTestingAddress.value = false
        }
    }

    private fun reconcilePendingChanges(serverState: PoolSystem) {
        val now = System.currentTimeMillis()
        synchronized(stateLock) {
            val iterator = _pendingChanges.iterator()
            while (iterator.hasNext()) {
                val change = iterator.next()
                if (change.verify(serverState)) {
                    iterator.remove()
                } else if (now - change.appliedAt > 5000) {
                    _rejections.tryEmit("${change.description} didn't take effect")
                    iterator.remove()
                }
            }
        }
    }

    private fun connectWebSocket() {
        val wsUrl = (baseUrl ?: return).replace("http://", "ws://") + "/api/ws"
        val request = Request.Builder().url(wsUrl).build()
        webSocket = okHttp.newWebSocket(request, object : WebSocketListener() {
            override fun onMessage(ws: WebSocket, text: String) {
                scope.launch {
                    val serverState = runCatching { poolSystemAdapter.fromJson(text) }.getOrNull()
                    if (serverState != null) {
                        applyServerState(serverState)
                        _connectionState.value = ConnectionState.CONNECTED
                        reconnectDelay.set(1000L)
                        recordDiagnostic("websocket", "Applied daemon state snapshot")
                    } else {
                        recordDiagnostic("websocket", "Failed to decode daemon state snapshot")
                        refresh()
                    }
                }
            }
            override fun onFailure(ws: WebSocket, t: Throwable, response: Response?) {
                _connectionState.value = ConnectionState.DISCONNECTED
                recordDiagnostic("websocket", "WebSocket failed: ${t.message}")
                reconnectJob = scope.launch {
                    delay(reconnectDelay.get())
                    reconnectDelay.getAndUpdate { (it * 2).coerceAtMost(30000L) }
                    connectWebSocket()
                }
            }
            override fun onClosed(ws: WebSocket, code: Int, reason: String) {
                _connectionState.value = ConnectionState.DISCONNECTED
                recordDiagnostic("websocket", "WebSocket closed")
            }
        })
        recordDiagnostic("websocket", "Connected to $wsUrl")
    }

    // Lifecycle: disconnect WS on background, reconnect on foreground
    override fun onStart(owner: LifecycleOwner) {
        connectJob = scope.launch {
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
        connectJob?.cancel()
        connectJob = null
        reconnectJob?.cancel()
        reconnectJob = null
        webSocket?.close(1000, "backgrounded")
        webSocket = null
    }

    // Action methods
    suspend fun setPoolMode(state: String) {
        val turnOn = state == "on"
        applyOptimistic(
            description = "Pool ${if (turnOn) "on" else "off"}",
            mutate = { sys ->
                sys.copy(
                    pool = sys.pool?.optimisticCommand(
                        on = turnOn,
                        sharedPump = sys.system.pool_spa_shared_pump,
                    )
                )
            },
            verify = { sys -> sys.pool?.on == turnOn },
        )

        try {
            if (turnOn) api?.poolOn() else api?.poolOff()
        } catch (e: Exception) {
            _rejections.tryEmit("Pool ${state} failed: ${e.message}")
            try { refresh() } catch (_: Exception) {}
            return
        }
        delay(1000)
        refresh()
    }

    suspend fun setSpaState(state: String) {
        val preMutationSpa = _state.value?.spa  // capture BEFORE applyOptimistic

        when (state) {
            "off" -> applyOptimistic(
                description = "Spa off",
                mutate = { sys ->
                    sys.copy(
                        spa = sys.spa?.optimisticCommand(
                            on = false,
                            accessories = sys.spa.accessories.mapValues { false },
                            sharedPump = sys.system.pool_spa_shared_pump,
                        )
                    )
                },
                verify = { sys -> sys.spa?.on == false },
            )
            "spa" -> applyOptimistic(
                description = "Spa on",
                mutate = { sys ->
                    sys.copy(
                        spa = sys.spa?.optimisticCommand(
                            on = true,
                            accessories = sys.spa.accessories.mapValues { false },
                            sharedPump = sys.system.pool_spa_shared_pump,
                        )
                    )
                },
                verify = { sys -> sys.spa?.on == true && sys.spa.accessories["jets"] != true },
            )
            "jets" -> applyOptimistic(
                description = "Jets on",
                mutate = { sys ->
                    sys.copy(
                        spa = sys.spa?.optimisticCommand(
                            on = true,
                            accessories = sys.spa.accessories + ("jets" to true),
                            sharedPump = sys.system.pool_spa_shared_pump,
                        )
                    )
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
                mutate = { sys -> sys.copy(pool = sys.pool?.optimisticSetpointChange(temp)) },
                verify = { sys -> sys.pool?.setpoint == temp },
            )
            "spa" -> applyOptimistic(
                description = "Spa setpoint $temp",
                mutate = { sys -> sys.copy(spa = sys.spa?.optimisticSetpointChange(temp)) },
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

    private fun recordDiagnostic(category: String, message: String) {
        android.util.Log.d("PoolRepo", "[$category] $message")
        _diagnostics.value = (_diagnostics.value + DiagnosticEvent(category = category, message = message))
            .takeLast(40)
    }

    private fun applyServerState(serverState: PoolSystem) {
        reconcilePendingChanges(serverState)
        synchronized(stateLock) {
            var merged = serverState
            for (change in _pendingChanges) {
                merged = change.mutate(merged)
            }
            _state.value = merged
        }
    }

    private suspend fun tryConnect(address: String): Boolean {
        ensureApi(address)
        _connectionState.value = ConnectionState.CONNECTING
        recordDiagnostic("startup", "Active daemon address set to $address")

        for (attempt in 1..3) {
            try {
                recordDiagnostic("http", "GET $address/api/pool")
                val result = api?.getPool() ?: throw IllegalStateException("API not ready")
                applyServerState(result)
                _connectionState.value = ConnectionState.CONNECTED
                reconnectDelay.set(1000L)
                recordDiagnostic("http", "GET /api/pool succeeded")
                connectWebSocket()
                return true
            } catch (e: Exception) {
                recordDiagnostic("http", "Connect attempt $attempt failed: ${e.message}")
                if (attempt < 3) delay(2000L * attempt)
            }
        }

        return false
    }

    private fun ensureApi(address: String) {
        if (address == baseUrl && api != null) return
        webSocket?.close(1000, "switching")
        webSocket = null
        baseUrl = address
        _activeAddress.value = address
        api = buildApi(address)
    }

    private fun buildApi(address: String): PoolApiClient {
        val retrofitBase = if (address.endsWith("/")) address else "$address/"
        return Retrofit.Builder()
            .baseUrl(retrofitBase)
            .client(okHttp)
            .addConverterFactory(MoshiConverterFactory.create(moshi))
            .build()
            .create(PoolApiClient::class.java)
    }

    private fun normalizeAddress(raw: String): String? {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return null

        val withScheme = if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) {
            trimmed
        } else {
            "http://$trimmed"
        }

        val parsed = withScheme.toHttpUrlOrNull() ?: return null
        return parsed.newBuilder()
            .encodedPath("/")
            .query(null)
            .fragment(null)
            .build()
            .toString()
            .removeSuffix("/")
    }
}
