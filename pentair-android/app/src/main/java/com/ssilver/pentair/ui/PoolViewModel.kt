package com.ssilver.pentair.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ssilver.pentair.data.PoolRepository
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltViewModel
class PoolViewModel @Inject constructor(
    private val repository: PoolRepository,
) : ViewModel() {

    val state = repository.state
    val connectionState = repository.connectionState

    init {
        viewModelScope.launch { repository.connect() }
    }

    fun setSpaState(s: String) = viewModelScope.launch { repository.setSpaState(s) }

    fun setLightMode(m: String) = viewModelScope.launch { repository.setLightMode(m) }

    fun setSetpoint(body: String, temp: Int) = viewModelScope.launch { repository.setSetpoint(body, temp) }

    fun toggleAux(id: String, on: Boolean) = viewModelScope.launch { repository.toggleAux(id, on) }
}
