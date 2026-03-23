package com.ssilver.pentair.discovery

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject
import javax.inject.Singleton
import kotlin.coroutines.resume

@Singleton
class DaemonDiscovery @Inject constructor(
    @ApplicationContext private val context: Context,
) {
    private val prefs = context.getSharedPreferences("pentair", Context.MODE_PRIVATE)

    suspend fun discover(): String? = withContext(Dispatchers.IO) {
        withTimeoutOrNull(5000L) {
            suspendCancellableCoroutine { cont ->
                val nsdManager = context.getSystemService(Context.NSD_SERVICE) as NsdManager
                val listenerHolder = arrayOfNulls<NsdManager.DiscoveryListener>(1)

                val discoveryListener = object : NsdManager.DiscoveryListener {
                    override fun onDiscoveryStarted(serviceType: String) {}
                    override fun onDiscoveryStopped(serviceType: String) {}
                    override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                        if (cont.isActive) cont.resume(null)
                    }
                    override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
                    override fun onServiceLost(serviceInfo: NsdServiceInfo) {}

                    override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                        if (serviceInfo.serviceType == "_pentair._tcp.") {
                            nsdManager.resolveService(serviceInfo, object : NsdManager.ResolveListener {
                                override fun onResolveFailed(si: NsdServiceInfo, errorCode: Int) {
                                    if (cont.isActive) cont.resume(null)
                                }
                                override fun onServiceResolved(si: NsdServiceInfo) {
                                    val addr = resolvedAddress(si)
                                    if (addr == null) {
                                        if (cont.isActive) cont.resume(null)
                                        return
                                    }
                                    prefs.edit().putString("daemon_address", addr).apply()
                                    if (cont.isActive) cont.resume(addr)
                                    try {
                                        listenerHolder[0]?.let { nsdManager.stopServiceDiscovery(it) }
                                    } catch (_: Exception) {}
                                }
                            })
                        }
                    }
                }
                listenerHolder[0] = discoveryListener

                nsdManager.discoverServices("_pentair._tcp", NsdManager.PROTOCOL_DNS_SD, discoveryListener)

                cont.invokeOnCancellation {
                    try { nsdManager.stopServiceDiscovery(discoveryListener) } catch (_: Exception) {}
                }
            }
        } ?: cachedAddress()
    }

    fun cachedAddress(): String? = prefs.getString("daemon_address", null)

    fun setManualAddress(address: String) {
        prefs.edit().putString("daemon_address", address).apply()
    }

    fun connectionCandidates(discoveredAddress: String?): List<String> {
        val candidates = linkedSetOf<String>()
        discoveredAddress?.let(candidates::add)
        cachedAddress()?.let(candidates::add)

        if (isProbablyEmulator()) {
            candidates += "http://10.0.2.2:8080"
        }

        return candidates.toList()
    }

    private fun resolvedAddress(serviceInfo: NsdServiceInfo): String? {
        val host = serviceInfo.host ?: return null
        val ipv4Host = serviceInfo.hostAddresses
            .asSequence()
            .mapNotNull { it.hostAddress?.trim() }
            .firstOrNull { it.isNotEmpty() && !it.contains(':') }

        val namedHost = host.hostName
            ?.trim()
            ?.takeIf { it.isNotEmpty() && it != host.hostAddress }

        val preferredHost = ipv4Host
            ?: namedHost
            ?: host.hostAddress
            ?: return null

        val urlHost = when {
            preferredHost.contains(":") && !preferredHost.startsWith("[") -> {
                "[${preferredHost.substringBefore('%')}]"
            }
            else -> preferredHost
        }

        return "http://$urlHost:${serviceInfo.port}"
    }

    private fun isProbablyEmulator(): Boolean =
        Build.FINGERPRINT.startsWith("generic") ||
            Build.FINGERPRINT.contains("emulator", ignoreCase = true) ||
            Build.MODEL.contains("Emulator", ignoreCase = true) ||
            Build.MODEL.contains("sdk_gphone", ignoreCase = true) ||
            Build.HARDWARE.contains("ranchu", ignoreCase = true) ||
            Build.PRODUCT.contains("sdk", ignoreCase = true)
}
