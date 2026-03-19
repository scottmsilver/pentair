package com.ssilver.pentair.data

import android.content.Context
import com.ssilver.pentair.discovery.DaemonDiscovery
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

class DeviceTokenManager(private val context: Context) {
    private val prefs = context.getSharedPreferences("pentair", Context.MODE_PRIVATE)

    suspend fun register(token: String) = withContext(Dispatchers.IO) {
        // Save token locally
        prefs.edit().putString("fcm_token", token).apply()

        // Send to daemon
        val baseUrl = prefs.getString("daemon_address", null) ?: return@withContext

        try {
            val json = """{"token":"$token"}"""
            val body = json.toRequestBody("application/json".toMediaType())
            val request = Request.Builder()
                .url("$baseUrl/api/devices/register")
                .post(body)
                .build()
            OkHttpClient().newCall(request).execute().close()
        } catch (_: Exception) {
            // Will retry on next app launch
        }
    }
}
