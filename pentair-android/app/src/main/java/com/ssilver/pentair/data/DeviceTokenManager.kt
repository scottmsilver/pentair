package com.ssilver.pentair.data

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class DeviceTokenManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val okHttp: OkHttpClient,
) {
    private val prefs = context.getSharedPreferences("pentair", Context.MODE_PRIVATE)

    suspend fun register(token: String) = withContext(Dispatchers.IO) {
        // Save token locally
        prefs.edit().putString("fcm_token", token).apply()

        // Send to daemon
        val baseUrl = prefs.getString("daemon_address", null) ?: return@withContext

        try {
            val json = JSONObject().apply {
                put("token", token)
            }.toString()
            val body = json.toRequestBody("application/json".toMediaType())
            val request = Request.Builder()
                .url("$baseUrl/api/devices/register")
                .post(body)
                .build()
            okHttp.newCall(request).execute().close()
        } catch (_: Exception) {
            // Will retry on next app launch
        }
    }
}
