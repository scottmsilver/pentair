package com.ssilver.pentair.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ssilver.pentair.data.PoolRepository
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltViewModel
class PoolViewModel @Inject constructor(
    private val repository: PoolRepository,
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

    init {
        viewModelScope.launch { repository.connect() }
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
