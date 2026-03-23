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

    override fun onMessageReceived(message: RemoteMessage) {
        message.notification?.let {
            NotificationHelper.show(
                this,
                it.title ?: "Pool",
                it.body ?: ""
            )
        }
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
