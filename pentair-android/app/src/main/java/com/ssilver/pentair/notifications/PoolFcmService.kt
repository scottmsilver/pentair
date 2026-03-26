package com.ssilver.pentair.notifications

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import com.ssilver.pentair.data.DeviceTokenManager
import dagger.hilt.android.AndroidEntryPoint
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import javax.inject.Inject

@AndroidEntryPoint
class PoolFcmService : FirebaseMessagingService() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Inject lateinit var deviceTokenManager: DeviceTokenManager
    @Inject lateinit var spaHeatLiveUpdate: SpaHeatLiveUpdate

    override fun onMessageReceived(message: RemoteMessage) {
        val data = message.data
        if (data["kind"] == "spa_heat") {
            handleSpaHeatMessage(data)
        } else {
            message.notification?.let {
                NotificationHelper.show(
                    this,
                    it.title ?: "Pool",
                    it.body ?: ""
                )
            }
        }
    }

    private fun handleSpaHeatMessage(data: Map<String, String>) {
        val spaHeatData = SpaHeatData(
            currentTempF = data["current_temp_f"]?.toIntOrNull() ?: return,
            targetTempF = data["target_temp_f"]?.toIntOrNull() ?: return,
            startTempF = data["start_temp_f"]?.toIntOrNull() ?: 0,
            progressPct = data["progress_pct"]?.toIntOrNull()?.coerceIn(0, 100) ?: 0,
            minutesRemaining = data["minutes_remaining"]?.toIntOrNull(),
            phase = data["phase"],
            milestone = data["milestone"],
            sessionId = data["session_id"]
        )

        spaHeatLiveUpdate.update(spaHeatData)
    }

    override fun onNewToken(token: String) {
        serviceScope.launch {
            deviceTokenManager.register(token)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        serviceScope.cancel()
    }
}
