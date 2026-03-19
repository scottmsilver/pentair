package com.ssilver.pentair.data

import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import com.ssilver.pentair.discovery.DaemonDiscovery
import com.squareup.moshi.Moshi
import com.squareup.moshi.kotlin.reflect.KotlinJsonAdapterFactory
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
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

@Singleton
class PoolRepository @Inject constructor(
    private val okHttp: OkHttpClient,
    private val discovery: DaemonDiscovery,
) : DefaultLifecycleObserver {

    private val _state = MutableStateFlow<PoolSystem?>(null)
    val state: StateFlow<PoolSystem?> = _state.asStateFlow()

    private val _connectionState = MutableStateFlow(ConnectionState.DISCOVERING)
    val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    private var api: PoolApiClient? = null
    private var webSocket: WebSocket? = null
    private var baseUrl: String? = null
    private var reconnectDelay = 1000L
    private val scope = CoroutineScope(Dispatchers.IO)

    suspend fun connect() {
        _connectionState.value = ConnectionState.DISCOVERING
        val addr = discovery.discover() ?: discovery.cachedAddress()
        if (addr == null) {
            _connectionState.value = ConnectionState.DISCONNECTED
            return
        }
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
            _state.value = api?.getPool()
            _connectionState.value = ConnectionState.CONNECTED
            reconnectDelay = 1000L // Reset on success
        } catch (e: Exception) {
            _connectionState.value = ConnectionState.DISCONNECTED
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
        when (state) {
            "off" -> api?.spaOff()
            "spa" -> {
                val current = _state.value?.spa
                if (current?.accessories?.get("jets") == true) api?.jetsOff()
                if (current?.on != true) api?.spaOn()
            }
            "jets" -> {
                if (_state.value?.spa?.on != true) api?.spaOn()
                api?.jetsOn()
            }
        }
        delay(1500)
        refresh()
    }

    suspend fun setLightMode(mode: String) {
        if (mode == "off") {
            api?.lightsOff()
        } else {
            api?.lightsMode(mapOf("mode" to mode))
        }
        delay(500)
        refresh()
    }

    suspend fun setSetpoint(body: String, temp: Int) {
        when (body) {
            "pool" -> api?.poolHeat(mapOf("setpoint" to temp))
            "spa" -> api?.spaHeat(mapOf("setpoint" to temp))
        }
        delay(2000)
        refresh()
    }

    suspend fun toggleAux(id: String, on: Boolean) {
        if (on) api?.auxOn(id) else api?.auxOff(id)
        delay(1000)
        refresh()
    }
}
