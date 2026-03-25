package com.ssilver.pentair.data

import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.suspendCancellableCoroutine
import javax.inject.Inject
import javax.inject.Singleton
import kotlin.coroutines.resume

interface MessagingTokenProvider {
    suspend fun currentToken(): String?
}

@Singleton
class FirebaseMessagingTokenProvider @Inject constructor() : MessagingTokenProvider {
    override suspend fun currentToken(): String? = suspendCancellableCoroutine { cont ->
        FirebaseMessaging.getInstance().token
            .addOnCompleteListener { task ->
                if (!cont.isActive) return@addOnCompleteListener
                cont.resume(task.result)
            }
            .addOnFailureListener {
                if (cont.isActive) cont.resume(null)
            }
    }
}
