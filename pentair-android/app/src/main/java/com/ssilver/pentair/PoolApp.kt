package com.ssilver.pentair

import android.app.Application
import androidx.lifecycle.ProcessLifecycleOwner
import com.ssilver.pentair.data.PoolRepository
import dagger.hilt.android.HiltAndroidApp
import javax.inject.Inject

@HiltAndroidApp
class PoolApp : Application() {
    @Inject lateinit var repository: PoolRepository

    override fun onCreate() {
        super.onCreate()
        ProcessLifecycleOwner.get().lifecycle.addObserver(repository)
    }
}
