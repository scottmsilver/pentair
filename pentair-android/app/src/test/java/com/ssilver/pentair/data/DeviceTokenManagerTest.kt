package com.ssilver.pentair.data

import android.content.Context
import android.content.SharedPreferences
import android.content.ContextWrapper
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

class DeviceTokenManagerTest {
    @Test
    fun `ensureRegistered uploads current token when daemon address is known`() = runTest {
        val prefs = InMemorySharedPreferences(
            mutableMapOf("daemon_address" to "http://daemon:8080")
        )
        val context = object : ContextWrapper(null) {
            override fun getSharedPreferences(name: String?, mode: Int): SharedPreferences = prefs
        }

        val requests = mutableListOf<String>()
        val registrationClient = object : DeviceRegistrationClient {
            override suspend fun register(baseUrl: String, token: String) {
                requests += "$baseUrl/api/devices/register|{\"token\":\"$token\"}"
            }
        }

        val tokenProvider = object : MessagingTokenProvider {
            override suspend fun currentToken(): String? = "token-123"
        }

        val manager = DeviceTokenManager(context, registrationClient, tokenProvider)

        manager.ensureRegistered()

        assertEquals(
            listOf("http://daemon:8080/api/devices/register|{\"token\":\"token-123\"}"),
            requests,
        )
    }

    private class InMemorySharedPreferences(
        private val values: MutableMap<String, String?> = mutableMapOf(),
    ) : SharedPreferences {
        override fun getString(key: String?, defValue: String?): String? = values[key] ?: defValue
        override fun edit(): SharedPreferences.Editor = Editor(values)

        override fun getAll(): MutableMap<String, *> = values
        override fun getStringSet(key: String?, defValues: MutableSet<String>?): MutableSet<String>? = defValues
        override fun getInt(key: String?, defValue: Int): Int = defValue
        override fun getLong(key: String?, defValue: Long): Long = defValue
        override fun getFloat(key: String?, defValue: Float): Float = defValue
        override fun getBoolean(key: String?, defValue: Boolean): Boolean = defValue
        override fun contains(key: String?): Boolean = values.containsKey(key)
        override fun registerOnSharedPreferenceChangeListener(listener: SharedPreferences.OnSharedPreferenceChangeListener?) = Unit
        override fun unregisterOnSharedPreferenceChangeListener(listener: SharedPreferences.OnSharedPreferenceChangeListener?) = Unit

        private class Editor(
            private val values: MutableMap<String, String?>,
        ) : SharedPreferences.Editor {
            override fun putString(key: String?, value: String?): SharedPreferences.Editor {
                if (key != null) values[key] = value
                return this
            }

            override fun apply() = Unit
            override fun commit(): Boolean = true
            override fun clear(): SharedPreferences.Editor {
                values.clear()
                return this
            }
            override fun remove(key: String?): SharedPreferences.Editor {
                if (key != null) values.remove(key)
                return this
            }
            override fun putStringSet(key: String?, values: MutableSet<String>?): SharedPreferences.Editor = this
            override fun putInt(key: String?, value: Int): SharedPreferences.Editor = this
            override fun putLong(key: String?, value: Long): SharedPreferences.Editor = this
            override fun putFloat(key: String?, value: Float): SharedPreferences.Editor = this
            override fun putBoolean(key: String?, value: Boolean): SharedPreferences.Editor = this
        }
    }
}
