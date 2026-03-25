package com.ssilver.pentair.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import javax.inject.Inject
import javax.inject.Singleton

interface DeviceRegistrationClient {
    suspend fun register(baseUrl: String, token: String)
}

@Singleton
class OkHttpDeviceRegistrationClient @Inject constructor(
    private val okHttp: OkHttpClient,
) : DeviceRegistrationClient {
    override suspend fun register(baseUrl: String, token: String) = withContext(Dispatchers.IO) {
        val json = JSONObject().apply {
            put("token", token)
        }.toString()
        val body = json.toRequestBody("application/json".toMediaType())
        val request = Request.Builder()
            .url("$baseUrl/api/devices/register")
            .post(body)
            .build()
        val response = okHttp.newCall(request).execute()
        response.use {
            if (!it.isSuccessful) throw java.io.IOException("Registration failed: ${it.code}")
        }
    }
}
