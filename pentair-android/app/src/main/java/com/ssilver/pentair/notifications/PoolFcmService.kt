package com.ssilver.pentair.notifications

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import com.ssilver.pentair.data.DeviceTokenManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

class PoolFcmService : FirebaseMessagingService() {
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
        CoroutineScope(Dispatchers.IO).launch {
            DeviceTokenManager(applicationContext).register(token)
        }
    }
}
