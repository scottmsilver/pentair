# Phase 2: Daemon Additions + Android App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add mDNS discovery, FCM push notifications, and device registration to the daemon, then build a native Android app (Kotlin + Compose) that mirrors the web UI.

**Architecture:** The daemon gets three additions (mDNS advertisement, FCM push sender, device token endpoint). The Android app is a new Gradle project consuming the daemon's existing semantic REST API via Retrofit, with mDNS discovery, WebSocket live updates, and FCM notification receiving.

**Tech Stack:** Rust (daemon additions), Kotlin + Jetpack Compose (Android), Retrofit + OkHttp + Moshi (networking), Hilt (DI), Firebase Cloud Messaging (notifications)

**Note:** The Matter bridge (Google Home / HomeKit / Alexa) is deferred to a separate plan pending `rs-matter` crate evaluation. This covers spec Components 1, 3, and 4. Component 2 (Matter) will be a separate plan.

**Implementation Notes (from plan review):**
- Task 4: Add stub `PoolScreen` and `PoolTheme` composables so project compiles before Tasks 8/11. Add `google-services.json` setup instructions and `com.google.gms.google-services` Gradle plugin.
- Task 3: Add FCM error handling for 401 (refresh + retry), 429 (backoff), 500 (log). Add "connection lost" notification trigger when adapter TCP drops.
- Task 6: Wrap `suspendCancellableCoroutine` in `withTimeout(5000)` and fix `this@object` syntax.
- Task 7: Add exponential backoff to WebSocket reconnect. Add optimistic update pattern to action methods.
- Task 11: Add manual daemon address entry field to SettingsDrawer.
- Task 12: Provide complete `DeviceTokenManager.kt` implementation. Add per-event notification channel importance.

---

## Part A: Daemon Additions

### Task 1: mDNS Service Advertisement

**Files:**
- Modify: `pentair-daemon/Cargo.toml`
- Modify: `pentair-daemon/src/main.rs`

- [ ] **Step 1: Add mdns-sd dependency**

Add to `pentair-daemon/Cargo.toml`:
```toml
mdns-sd = "0.11"
```

- [ ] **Step 2: Add mDNS registration after HTTP server bind**

In `pentair-daemon/src/main.rs`, after `let listener = ...`, add:
```rust
// Advertise via mDNS for app discovery
let mdns = mdns_sd::ServiceDaemon::new().expect("failed to start mDNS");
let hostname = hostname::get()
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();
let bind_port = listener.local_addr()?.port();
let service_info = mdns_sd::ServiceInfo::new(
    "_pentair._tcp.local.",
    "Pentair Pool",
    &format!("{}.local.", hostname),
    "",
    bind_port,
    None,
).expect("failed to create mDNS service");
mdns.register(service_info).expect("failed to register mDNS service");
info!("mDNS: advertising _pentair._tcp on port {}", bind_port);
```

Also add `hostname = "0.4"` to Cargo.toml dependencies.

- [ ] **Step 3: Build and verify**

Run: `cargo build -p pentair-daemon`
Expected: Compiles clean.

- [ ] **Step 4: Test mDNS discovery manually**

Start the daemon, then from another terminal:
```bash
avahi-browse -r _pentair._tcp 2>/dev/null || dns-sd -B _pentair._tcp 2>/dev/null
```
Expected: Service "Pentair Pool" appears with correct port.

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/Cargo.toml pentair-daemon/src/main.rs
git commit -m "feat(daemon): add mDNS service advertisement for app discovery"
```

---

### Task 2: Device Token Registration Endpoint

**Files:**
- Modify: `pentair-daemon/src/state.rs`
- Create: `pentair-daemon/src/devices.rs`
- Modify: `pentair-daemon/src/api/routes.rs`
- Modify: `pentair-daemon/src/main.rs`

- [ ] **Step 1: Create devices.rs for token storage**

```rust
// pentair-daemon/src/devices.rs
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DeviceStore {
    tokens: Vec<String>,
}

#[derive(Clone)]
pub struct DeviceManager {
    store: Arc<RwLock<DeviceStore>>,
    path: PathBuf,
}

impl DeviceManager {
    pub fn load(path: PathBuf) -> Self {
        let store = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(_) => DeviceStore::default(),
            }
        } else {
            DeviceStore::default()
        };
        info!("loaded {} device token(s) from {:?}", store.tokens.len(), path);
        Self {
            store: Arc::new(RwLock::new(store)),
            path,
        }
    }

    pub async fn register(&self, token: String) {
        let mut store = self.store.write().await;
        if !store.tokens.contains(&token) {
            store.tokens.push(token);
            self.persist(&store);
            info!("registered new device token ({} total)", store.tokens.len());
        }
    }

    pub async fn remove(&self, token: &str) {
        let mut store = self.store.write().await;
        let before = store.tokens.len();
        store.tokens.retain(|t| t != token);
        if store.tokens.len() < before {
            self.persist(&store);
            info!("removed invalid device token ({} remaining)", store.tokens.len());
        }
    }

    pub async fn tokens(&self) -> Vec<String> {
        self.store.read().await.tokens.clone()
    }

    fn persist(&self, store: &DeviceStore) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(store) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.path, json) {
                    warn!("failed to persist device tokens: {}", e);
                }
            }
            Err(e) => warn!("failed to serialize device tokens: {}", e),
        }
    }
}
```

- [ ] **Step 2: Add module and wire into main.rs**

Add `mod devices;` to `main.rs`. Create DeviceManager before starting the server:
```rust
let devices_path = dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("."))
    .join(".pentair")
    .join("devices.json");
let devices = devices::DeviceManager::load(devices_path);
```

Add `dirs = "5"` to Cargo.toml.

Pass `devices` to the router: update `api::create_router` to accept it.

- [ ] **Step 3: Add register endpoint to routes.rs**

Add to the router:
```rust
.route("/api/devices/register", post(register_device))
```

Add `DeviceManager` to `AppState`:
```rust
pub struct AppState {
    pub shared: SharedState,
    pub cmd_tx: mpsc::Sender<AdapterCommand>,
    pub push_tx: broadcast::Sender<PushEvent>,
    pub devices: DeviceManager,
}
```

Add handler:
```rust
#[derive(Deserialize)]
struct RegisterRequest { token: String }

async fn register_device(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Json<serde_json::Value> {
    state.devices.register(body.token).await;
    Json(serde_json::json!({"ok": true}))
}
```

- [ ] **Step 4: Build and test**

```bash
cargo build -p pentair-daemon
# Start daemon, then:
curl -s -X POST -H 'Content-Type: application/json' \
  -d '{"token":"test-token-123"}' \
  http://localhost:8080/api/devices/register
# Expected: {"ok": true}
# Check: cat ~/.pentair/devices.json should contain the token
```

- [ ] **Step 5: Commit**

```bash
git add pentair-daemon/
git commit -m "feat(daemon): add device token registration endpoint for FCM"
```

---

### Task 3: FCM Push Sender

**Files:**
- Create: `pentair-daemon/src/fcm.rs`
- Modify: `pentair-daemon/Cargo.toml`
- Modify: `pentair-daemon/src/config.rs`
- Modify: `pentair-daemon/src/adapter.rs`
- Modify: `pentair-daemon/src/main.rs`

- [ ] **Step 1: Add dependencies**

Add to `pentair-daemon/Cargo.toml`:
```toml
reqwest = { version = "0.12", features = ["json"] }
jsonwebtoken = "9"
```

- [ ] **Step 2: Add FCM config section**

In `config.rs`, add:
```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FcmConfig {
    #[serde(default)]
    pub service_account: String,
    #[serde(default)]
    pub project_id: String,
}
```

Add `#[serde(default)] pub fcm: FcmConfig` to `Config` struct and to the `Default` impl.

- [ ] **Step 3: Create fcm.rs — OAuth2 token + push sender**

```rust
// pentair-daemon/src/fcm.rs
use crate::devices::DeviceManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    token_uri: String,
}

struct TokenCache {
    access_token: String,
    expires_at: std::time::Instant,
}

pub struct FcmSender {
    project_id: String,
    service_account: ServiceAccount,
    token_cache: RwLock<Option<TokenCache>>,
    http: reqwest::Client,
    devices: DeviceManager,
}

#[derive(Serialize)]
struct FcmMessage {
    message: FcmMessageBody,
}

#[derive(Serialize)]
struct FcmMessageBody {
    token: String,
    notification: FcmNotification,
}

#[derive(Serialize)]
struct FcmNotification {
    title: String,
    body: String,
}

impl FcmSender {
    pub fn new(
        project_id: String,
        service_account_path: &str,
        devices: DeviceManager,
    ) -> Option<Self> {
        if service_account_path.is_empty() || project_id.is_empty() {
            info!("FCM not configured (no service account or project ID)");
            return None;
        }
        let contents = match std::fs::read_to_string(service_account_path) {
            Ok(c) => c,
            Err(e) => {
                error!("failed to read FCM service account: {}", e);
                return None;
            }
        };
        let sa: ServiceAccount = match serde_json::from_str(&contents) {
            Ok(sa) => sa,
            Err(e) => {
                error!("failed to parse FCM service account: {}", e);
                return None;
            }
        };
        info!("FCM configured for project {}", project_id);
        Some(Self {
            project_id,
            service_account: sa,
            token_cache: RwLock::new(None),
            http: reqwest::Client::new(),
            devices,
        })
    }

    async fn get_access_token(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check cache
        {
            let cache = self.token_cache.read().await;
            if let Some(tc) = cache.as_ref() {
                if tc.expires_at > std::time::Instant::now() {
                    return Ok(tc.access_token.clone());
                }
            }
        }

        // Generate JWT
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let claims = serde_json::json!({
            "iss": self.service_account.client_email,
            "scope": "https://www.googleapis.com/auth/firebase.messaging",
            "aud": self.service_account.token_uri,
            "iat": now,
            "exp": now + 3600,
        });

        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(
            self.service_account.private_key.as_bytes()
        )?;
        let jwt = jsonwebtoken::encode(&header, &claims, &key)?;

        // Exchange for access token
        let resp = self.http
            .post(&self.service_account.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await?;

        #[derive(Deserialize)]
        struct TokenResponse { access_token: String }
        let token_resp: TokenResponse = resp.json().await?;

        // Cache it (expires in ~55 min to be safe)
        let mut cache = self.token_cache.write().await;
        *cache = Some(TokenCache {
            access_token: token_resp.access_token.clone(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(3300),
        });

        Ok(token_resp.access_token)
    }

    pub async fn send(&self, title: &str, body: &str) {
        let tokens = self.devices.tokens().await;
        if tokens.is_empty() {
            return;
        }

        let access_token = match self.get_access_token().await {
            Ok(t) => t,
            Err(e) => {
                error!("FCM auth failed: {}", e);
                return;
            }
        };

        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );

        for token in &tokens {
            let msg = FcmMessage {
                message: FcmMessageBody {
                    token: token.clone(),
                    notification: FcmNotification {
                        title: title.to_string(),
                        body: body.to_string(),
                    },
                },
            };

            match self.http
                .post(&url)
                .bearer_auth(&access_token)
                .json(&msg)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        info!("FCM push sent: {}", title);
                    } else if status.as_u16() == 404 || status.as_u16() == 410 {
                        warn!("FCM token invalid, removing");
                        self.devices.remove(token).await;
                    } else {
                        warn!("FCM push failed: {} {}", status, resp.text().await.unwrap_or_default());
                    }
                }
                Err(e) => error!("FCM request failed: {}", e),
            }
        }
    }
}
```

- [ ] **Step 4: Add event detection to adapter.rs**

Add a `previous_pool_system: Option<PoolSystem>` field tracking. After each `refresh_status`, compare current vs previous to detect transitions. When a transition is detected, call `fcm_sender.send(title, body)`.

Key transitions to detect:
- Spa ready: `spa.on && spa.temperature >= spa.setpoint` was false, now true
- Freeze: `system.freeze_protection` was false, now true
- Heater started: `spa.heating` or `pool.heating` went from "off" to non-"off"

Wire the `FcmSender` (wrapped in `Option<Arc<FcmSender>>`) through from `main.rs` to the adapter task.

- [ ] **Step 5: Build and verify**

```bash
cargo build -p pentair-daemon
```
Expected: Compiles clean. FCM sender is `None` when not configured (no crash).

- [ ] **Step 6: Commit**

```bash
git add pentair-daemon/
git commit -m "feat(daemon): add FCM push notifications with OAuth2 auth"
```

---

## Part B: Android App

### Task 4: Android Project Scaffold

**Files:**
- Create: `pentair-android/` (full Gradle project)

- [ ] **Step 1: Create the Android project**

Use the Android Studio project structure. Create manually or via command line:

```
pentair-android/
  build.gradle.kts          (project-level)
  settings.gradle.kts
  gradle.properties
  app/
    build.gradle.kts        (app-level with Compose, Hilt, Retrofit, FCM deps)
    src/main/
      AndroidManifest.xml
      java/com/ssilver/pentair/
        PoolApp.kt
        MainActivity.kt
      res/
        values/
          strings.xml
          colors.xml
          themes.xml
```

Key dependencies in `app/build.gradle.kts`:
```kotlin
// Compose BOM
implementation(platform("androidx.compose:compose-bom:2024.12.01"))
implementation("androidx.compose.ui:ui")
implementation("androidx.compose.material3:material3")
implementation("androidx.activity:activity-compose:1.9.3")

// Networking
implementation("com.squareup.retrofit2:retrofit:2.11.0")
implementation("com.squareup.retrofit2:converter-moshi:2.11.0")
implementation("com.squareup.okhttp3:okhttp:4.12.0")
implementation("com.squareup.moshi:moshi-kotlin:1.15.1")

// Hilt
implementation("com.google.dagger:hilt-android:2.51.1")
kapt("com.google.dagger:hilt-compiler:2.51.1")

// Firebase
implementation(platform("com.google.firebase:firebase-bom:33.7.0"))
implementation("com.google.firebase:firebase-messaging")

// Lifecycle
implementation("androidx.lifecycle:lifecycle-process:2.8.7")
implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
```

- [ ] **Step 2: Create PoolApp.kt and MainActivity.kt stubs**

```kotlin
// PoolApp.kt
@HiltAndroidApp
class PoolApp : Application()

// MainActivity.kt
@AndroidEntryPoint
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            PoolTheme {
                PoolScreen()
            }
        }
    }
}
```

- [ ] **Step 3: Build and verify**

```bash
cd pentair-android && ./gradlew assembleDebug
```
Expected: APK builds successfully.

- [ ] **Step 4: Commit**

```bash
git add pentair-android/
git commit -m "feat(android): scaffold Kotlin + Compose project"
```

---

### Task 5: Data Layer — API Client + Data Classes

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/data/PoolSystem.kt`
- Create: `app/src/main/java/com/ssilver/pentair/data/PoolApiClient.kt`
- Create: `app/src/main/java/com/ssilver/pentair/di/NetworkModule.kt`

- [ ] **Step 1: Create PoolSystem.kt — data classes matching /api/pool JSON**

```kotlin
data class PoolSystem(
    val pool: BodyState?,
    val spa: SpaState?,
    val lights: LightState?,
    val auxiliaries: List<AuxState>,
    val pump: PumpInfo?,
    val system: SystemInfo
)
data class BodyState(val on: Boolean, val temperature: Int, val setpoint: Int, val heat_mode: String, val heating: String)
data class SpaState(val on: Boolean, val temperature: Int, val setpoint: Int, val heat_mode: String, val heating: String, val accessories: Map<String, Boolean>)
data class LightState(val on: Boolean, val mode: String?, val available_modes: List<String>)
data class AuxState(val id: String, val name: String, val on: Boolean)
data class PumpInfo(val pump_type: String, val running: Boolean, val watts: Int, val rpm: Int, val gpm: Int)
data class SystemInfo(val controller: String, val firmware: String?, val temp_unit: String, val air_temperature: Int, val freeze_protection: Boolean, val pool_spa_shared_pump: Boolean)
data class ApiResponse(val ok: Boolean, val error: String? = null)
```

- [ ] **Step 2: Create PoolApiClient.kt — Retrofit interface**

```kotlin
interface PoolApiClient {
    @GET("/api/pool") suspend fun getPool(): PoolSystem
    @POST("/api/spa/on") suspend fun spaOn(@Body body: Map<String, Any> = emptyMap()): ApiResponse
    @POST("/api/spa/off") suspend fun spaOff(): ApiResponse
    @POST("/api/spa/heat") suspend fun spaHeat(@Body body: Map<String, Any>): ApiResponse
    @POST("/api/spa/jets/on") suspend fun jetsOn(): ApiResponse
    @POST("/api/spa/jets/off") suspend fun jetsOff(): ApiResponse
    @POST("/api/pool/on") suspend fun poolOn(): ApiResponse
    @POST("/api/pool/off") suspend fun poolOff(): ApiResponse
    @POST("/api/pool/heat") suspend fun poolHeat(@Body body: Map<String, Any>): ApiResponse
    @POST("/api/lights/on") suspend fun lightsOn(): ApiResponse
    @POST("/api/lights/off") suspend fun lightsOff(): ApiResponse
    @POST("/api/lights/mode") suspend fun lightsMode(@Body body: Map<String, Any>): ApiResponse
    @POST("/api/auxiliary/{id}/on") suspend fun auxOn(@Path("id") id: String): ApiResponse
    @POST("/api/auxiliary/{id}/off") suspend fun auxOff(@Path("id") id: String): ApiResponse
    @POST("/api/devices/register") suspend fun registerDevice(@Body body: Map<String, String>): ApiResponse
}
```

- [ ] **Step 3: Create NetworkModule.kt — Hilt DI**

```kotlin
@Module
@InstallIn(SingletonComponent::class)
object NetworkModule {
    @Provides @Singleton
    fun provideOkHttp(): OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(10, TimeUnit.SECONDS)
        .build()

    // Base URL is set dynamically after mDNS discovery
    // PoolRepository handles creating the Retrofit instance
}
```

- [ ] **Step 4: Build and verify**

```bash
cd pentair-android && ./gradlew assembleDebug
```

- [ ] **Step 5: Commit**

```bash
git add pentair-android/app/src/main/java/com/ssilver/pentair/data/
git add pentair-android/app/src/main/java/com/ssilver/pentair/di/
git commit -m "feat(android): data layer — API client and data classes"
```

---

### Task 6: mDNS Discovery

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/discovery/DaemonDiscovery.kt`

- [ ] **Step 1: Create DaemonDiscovery.kt**

```kotlin
class DaemonDiscovery(private val context: Context) {
    private val prefs = context.getSharedPreferences("pentair", Context.MODE_PRIVATE)

    suspend fun discover(): String? = withContext(Dispatchers.IO) {
        suspendCancellableCoroutine { cont ->
            val nsdManager = context.getSystemService(Context.NSD_SERVICE) as NsdManager
            val listener = object : NsdManager.DiscoveryListener {
                override fun onServiceFound(service: NsdServiceInfo) {
                    if (service.serviceType == "_pentair._tcp.") {
                        nsdManager.resolveService(service, object : NsdManager.ResolveListener {
                            override fun onResolveFailed(s: NsdServiceInfo, code: Int) {
                                if (cont.isActive) cont.resume(cachedAddress())
                            }
                            override fun onServiceResolved(s: NsdServiceInfo) {
                                val addr = "http://${s.host.hostAddress}:${s.port}"
                                prefs.edit().putString("daemon_address", addr).apply()
                                if (cont.isActive) cont.resume(addr)
                                nsdManager.stopServiceDiscovery(this@object)
                            }
                        })
                    }
                }
                // ... other required overrides (onStarted, onStopped, etc.)
            }
            nsdManager.discoverServices("_pentair._tcp", NsdManager.PROTOCOL_DNS_SD, listener)

            // Timeout: fall back to cached address after 5 seconds
            cont.invokeOnCancellation {
                try { nsdManager.stopServiceDiscovery(listener) } catch (_: Exception) {}
            }
        }
    }

    fun cachedAddress(): String? = prefs.getString("daemon_address", null)
}
```

- [ ] **Step 2: Build and verify**

```bash
cd pentair-android && ./gradlew assembleDebug
```

- [ ] **Step 3: Commit**

```bash
git add pentair-android/app/src/main/java/com/ssilver/pentair/discovery/
git commit -m "feat(android): mDNS daemon discovery via NsdManager"
```

---

### Task 7: Repository — State Management + WebSocket

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/data/PoolRepository.kt`

- [ ] **Step 1: Create PoolRepository.kt**

Combines HTTP fetching, WebSocket live updates, and lifecycle management:

```kotlin
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

    // Called on app start
    suspend fun connect() {
        _connectionState.value = ConnectionState.DISCOVERING
        val addr = discovery.discover() ?: discovery.cachedAddress()
        if (addr == null) {
            _connectionState.value = ConnectionState.DISCONNECTED
            return
        }
        baseUrl = addr
        api = Retrofit.Builder()
            .baseUrl(addr)
            .client(okHttp)
            .addConverterFactory(MoshiConverterFactory.create())
            .build()
            .create(PoolApiClient::class.java)

        refresh()
        connectWebSocket()
    }

    suspend fun refresh() {
        try {
            _state.value = api?.getPool()
            _connectionState.value = ConnectionState.CONNECTED
        } catch (e: Exception) {
            _connectionState.value = ConnectionState.DISCONNECTED
        }
    }

    private fun connectWebSocket() {
        val wsUrl = baseUrl?.replace("http://", "ws://") + "/api/ws"
        val request = Request.Builder().url(wsUrl).build()
        webSocket = okHttp.newWebSocket(request, object : WebSocketListener() {
            override fun onMessage(ws: WebSocket, text: String) {
                // StatusChanged -> refresh
                kotlinx.coroutines.GlobalScope.launch { refresh() }
            }
            override fun onFailure(ws: WebSocket, t: Throwable, resp: Response?) {
                // Reconnect after delay
                kotlinx.coroutines.GlobalScope.launch {
                    delay(3000)
                    connectWebSocket()
                }
            }
        })
    }

    // Lifecycle: disconnect WS on background, reconnect on foreground
    override fun onStart(owner: LifecycleOwner) { /* reconnect */ }
    override fun onStop(owner: LifecycleOwner) { webSocket?.close(1000, null) }

    // Action methods delegate to API
    suspend fun setSpaState(state: String) { /* off/spa/jets -> appropriate API call */ }
    suspend fun setLightMode(mode: String) { api?.lightsMode(mapOf("mode" to mode)) }
    suspend fun setSetpoint(body: String, temp: Int) { /* pool/spa heat call */ }
    suspend fun toggleAux(id: String, on: Boolean) { if (on) api?.auxOn(id) else api?.auxOff(id) }
}

enum class ConnectionState { DISCOVERING, CONNECTED, DISCONNECTED }
```

- [ ] **Step 2: Build and verify**

```bash
cd pentair-android && ./gradlew assembleDebug
```

- [ ] **Step 3: Commit**

```bash
git add pentair-android/app/src/main/java/com/ssilver/pentair/data/PoolRepository.kt
git commit -m "feat(android): pool repository with HTTP + WebSocket + lifecycle"
```

---

### Task 8: Theme + Colors

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/ui/theme/Color.kt`
- Create: `app/src/main/java/com/ssilver/pentair/ui/theme/Theme.kt`

- [ ] **Step 1: Create Color.kt matching web UI palette**

```kotlin
val PoolBackground = Color(0xFF0C1222)
val DeckGray = Color(0xFF2A3040)
val WaterOff = Color(0xFF162030)
val PoolBlue = Color(0xFF0A5A8C)
val PoolBlueLight = Color(0xFF1080B8)
val SpaTeal = Color(0xFF0E6E6E)
val SpaTealLight = Color(0xFF18A8A0)
val Accent = Color(0xFF38BDF8)
val Teal = Color(0xFF2DD4BF)
val Warm = Color(0xFFF97316)
val Gold = Color(0xFFEAB308)
val TextBright = Color(0xFFF8FAFC)
val TextDim = Color(0xFF94A3B8)
val TextFaint = Color(0x59FFFFFF)
```

- [ ] **Step 2: Create Theme.kt**

Dark-only theme using the pool colors.

- [ ] **Step 3: Commit**

```bash
git add pentair-android/app/src/main/java/com/ssilver/pentair/ui/theme/
git commit -m "feat(android): dark theme matching web UI color palette"
```

---

### Task 9: Pool Visual Canvas

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/ui/PoolVisualCanvas.kt`

- [ ] **Step 1: Create the custom Canvas composable**

Draws the pool shape with:
- Deck border (DeckGray rounded rect)
- Pool water (full area, PoolBlue gradient when on, WaterOff when off)
- Spa water (top-right inset, SpaTeal gradient when on)
- Temperature text overlaid on each water body
- Setpoint text below each temperature
- Heating indicator ("Heating" in orange when active)
- Pool and spa are separate rounded rects with a deck gap between them

```kotlin
@Composable
fun PoolVisualCanvas(
    pool: BodyState?,
    spa: SpaState?,
    onPoolSetpointClick: () -> Unit,
    onSpaSetpointClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Canvas with drawRoundRect for pool/spa shapes
    // drawText for temperatures
    // Animated shimmer effect on water when active
}
```

- [ ] **Step 2: Build and verify**

Run on emulator to verify visual appearance.

- [ ] **Step 3: Commit**

```bash
git add pentair-android/app/src/main/java/com/ssilver/pentair/ui/PoolVisualCanvas.kt
git commit -m "feat(android): custom Canvas pool/spa visual with water effects"
```

---

### Task 10: Spa Segmented Control + Light Picker

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/ui/SpaSegmentedControl.kt`
- Create: `app/src/main/java/com/ssilver/pentair/ui/LightPicker.kt`

- [ ] **Step 1: Create SpaSegmentedControl.kt**

Apple Home-style segmented control with three states: Off | Spa | Jets.
Active segment has a teal pill background.

```kotlin
@Composable
fun SpaSegmentedControl(
    currentState: String, // "off", "spa", "jets"
    onStateChange: (String) -> Unit,
)
```

- [ ] **Step 2: Create LightPicker.kt**

Collapsible color swatch row. Shows selected mode as a colored circle.
Tap to expand full strip of 12 color gradient circles + off button.
Tap a color to select and collapse.

```kotlin
@Composable
fun LightPicker(
    lights: LightState?,
    onModeSelect: (String) -> Unit,
)
```

- [ ] **Step 3: Commit**

```bash
git add pentair-android/app/src/main/java/com/ssilver/pentair/ui/
git commit -m "feat(android): segmented spa control and collapsible light picker"
```

---

### Task 11: Main Screen + Settings Drawer + Setpoint Sheet

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/ui/PoolScreen.kt`
- Create: `app/src/main/java/com/ssilver/pentair/ui/SettingsDrawer.kt`
- Create: `app/src/main/java/com/ssilver/pentair/ui/SetpointBottomSheet.kt`

- [ ] **Step 1: Create PoolScreen.kt — main composable**

Assembles all UI components:
```kotlin
@Composable
fun PoolScreen(viewModel: PoolViewModel = hiltViewModel()) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    // PoolVisualCanvas
    // SpaSegmentedControl (overlaid on spa area)
    // LightPicker (overlaid at bottom of pool)
    // Gear icon -> SettingsDrawer
    // SetpointBottomSheet (shown on setpoint tap)
}
```

- [ ] **Step 2: Create SettingsDrawer.kt**

Bottom sheet or modal with auxiliaries (toggle buttons) and system info (controller, firmware, pump stats, connection status, equipment pad temp).

- [ ] **Step 3: Create SetpointBottomSheet.kt**

Bottom sheet with large temperature display, +/- buttons, Cancel/Set actions.

- [ ] **Step 4: Create PoolViewModel**

```kotlin
@HiltViewModel
class PoolViewModel @Inject constructor(
    private val repository: PoolRepository,
) : ViewModel() {
    val state = repository.state
    fun setSpaState(s: String) = viewModelScope.launch { repository.setSpaState(s) }
    fun setLightMode(m: String) = viewModelScope.launch { repository.setLightMode(m) }
    fun setSetpoint(body: String, temp: Int) = viewModelScope.launch { repository.setSetpoint(body, temp) }
    fun toggleAux(id: String, on: Boolean) = viewModelScope.launch { repository.toggleAux(id, on) }
}
```

- [ ] **Step 5: Build and run on emulator**

```bash
cd pentair-android && ./gradlew installDebug
```

Verify the full UI renders with mock data or against the live daemon.

- [ ] **Step 6: Commit**

```bash
git add pentair-android/app/src/main/java/com/ssilver/pentair/
git commit -m "feat(android): main screen with pool visual, controls, and settings"
```

---

### Task 12: FCM Notifications (Android side)

**Files:**
- Create: `app/src/main/java/com/ssilver/pentair/notifications/PoolFcmService.kt`
- Create: `app/src/main/java/com/ssilver/pentair/notifications/NotificationHelper.kt`
- Modify: `AndroidManifest.xml`

- [ ] **Step 1: Create NotificationHelper.kt**

Creates notification channel "Pool Alerts" and builds notifications.

- [ ] **Step 2: Create PoolFcmService.kt**

```kotlin
class PoolFcmService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        message.notification?.let {
            NotificationHelper.show(this, it.title ?: "Pool", it.body ?: "")
        }
    }
    override fun onNewToken(token: String) {
        // Re-register with daemon
        CoroutineScope(Dispatchers.IO).launch {
            DeviceTokenManager(applicationContext).register(token)
        }
    }
}
```

- [ ] **Step 3: Create DeviceTokenManager.kt**

Sends the FCM token to the daemon on first launch and on token refresh.

- [ ] **Step 4: Register service in AndroidManifest.xml**

- [ ] **Step 5: Build and test**

```bash
cd pentair-android && ./gradlew assembleDebug
```

- [ ] **Step 6: Commit**

```bash
git add pentair-android/
git commit -m "feat(android): FCM push notification receiving and display"
```

---

### Task 13: End-to-End Integration Test

- [ ] **Step 1: Start daemon with mDNS**

```bash
cargo run -p pentair-daemon
```

- [ ] **Step 2: Install and launch Android app on emulator**

The app should:
1. Discover the daemon via mDNS (or use `10.0.2.2:8080` for emulator → host)
2. Display live pool data from `GET /api/pool`
3. Tap spa segmented control → see spa turn on in real time
4. Tap light color → see light mode change
5. Receive push notification when spa reaches setpoint

- [ ] **Step 3: Verify all controls work**

Test each control against live hardware and verify state updates via WebSocket.

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat: Phase 2 complete — Android app + daemon mDNS/FCM"
```
