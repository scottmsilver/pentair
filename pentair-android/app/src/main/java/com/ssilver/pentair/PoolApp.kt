package com.ssilver.pentair

import android.app.Application
import androidx.lifecycle.ProcessLifecycleOwner
import com.ssilver.pentair.data.DeviceTokenManager
import com.ssilver.pentair.data.PoolRepository
import dagger.hilt.android.HiltAndroidApp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltAndroidApp
class PoolApp : Application() {
    @Inject lateinit var repository: PoolRepository
    @Inject lateinit var deviceTokenManager: DeviceTokenManager

    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onCreate() {
        super.onCreate()
        ProcessLifecycleOwner.get().lifecycle.addObserver(repository)
        appScope.launch {
            deviceTokenManager.ensureRegistered()
        }
    }
}
