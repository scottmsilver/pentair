package com.ssilver.pentair.data

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class DeviceTokenManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val registrationClient: DeviceRegistrationClient,
    private val tokenProvider: MessagingTokenProvider,
) {
    private val prefs = context.getSharedPreferences("pentair", Context.MODE_PRIVATE)

    suspend fun register(token: String) = withContext(Dispatchers.IO) {
        // Save token locally
        prefs.edit().putString("fcm_token", token).apply()

        // Send to daemon
        val baseUrl = prefs.getString("daemon_address", null) ?: return@withContext

        try {
            registrationClient.register(baseUrl, token)
        } catch (e: Exception) {
            android.util.Log.w("DeviceTokenManager", "Registration failed", e)
            // Will retry on next app launch
        }
    }

    suspend fun ensureRegistered() = withContext(Dispatchers.IO) {
        val token = prefs.getString("fcm_token", null) ?: tokenProvider.currentToken() ?: return@withContext
        register(token)
    }
}
