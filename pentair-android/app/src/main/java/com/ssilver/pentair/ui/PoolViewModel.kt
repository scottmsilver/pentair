package com.ssilver.pentair.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ssilver.pentair.data.PoolRepository
import com.ssilver.pentair.data.PoolSystem
import com.ssilver.pentair.notifications.SpaHeatData
import com.ssilver.pentair.notifications.SpaHeatLiveUpdate
import com.ssilver.pentair.widget.SpaHeatWidgetUpdater
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltViewModel
class PoolViewModel @Inject constructor(
    private val repository: PoolRepository,
    private val spaHeatLiveUpdate: SpaHeatLiveUpdate,
    private val widgetUpdater: SpaHeatWidgetUpdater,
) : ViewModel() {

    val state = repository.state
    val connectionState = repository.connectionState
    val manualAddress = repository.manualAddress
    val discoveredAddress = repository.discoveredAddress
    val activeAddress = repository.activeAddress
    val isTestingAddress = repository.isTestingAddress
    val isRefreshing = repository.isRefreshing
    val diagnostics = repository.diagnostics
    val rejections: SharedFlow<String> = repository.rejections

    private var wasProgressActive = false

    init {
        viewModelScope.launch { repository.connect() }
        viewModelScope.launch {
            repository.state.filterNotNull().collect { system ->
                evaluateSpaHeatNotification(system)
                widgetUpdater.update(system)
            }
        }
    }

    private fun evaluateSpaHeatNotification(system: PoolSystem) {
        val progress = system.spa?.spa_heat_progress ?: return

        if (progress.active) {
            val data = SpaHeatData(
                currentTempF = progress.current_temp_f,
                targetTempF = progress.target_temp_f,
                startTempF = progress.start_temp_f ?: progress.current_temp_f,
                progressPct = progress.progress_pct,
                minutesRemaining = progress.minutes_remaining,
                phase = progress.phase,
                milestone = progress.milestone,
                sessionId = progress.session_id,
            )

            spaHeatLiveUpdate.update(data)
            wasProgressActive = true
        } else if (wasProgressActive) {
            spaHeatLiveUpdate.end(progress.current_temp_f)
            wasProgressActive = false
        }
    }

    fun setManualAddress(address: String) = repository.setManualAddressInput(address)

    fun refresh() = viewModelScope.launch { repository.refresh() }

    fun applyManualAddress() = viewModelScope.launch { repository.applyManualAddress() }

    fun useDiscoveredAddress() = viewModelScope.launch { repository.useDiscoveredAddress() }

    fun testManualAddress() = viewModelScope.launch { repository.testManualAddress() }

    fun setPoolMode(state: String) = viewModelScope.launch { repository.setPoolMode(state) }

    fun setSpaState(s: String) = viewModelScope.launch { repository.setSpaState(s) }

    fun setLightMode(m: String) = viewModelScope.launch { repository.setLightMode(m) }

    fun setSetpoint(body: String, temp: Int) = viewModelScope.launch { repository.setSetpoint(body, temp) }

    fun toggleAux(id: String, on: Boolean) = viewModelScope.launch { repository.toggleAux(id, on) }
}
