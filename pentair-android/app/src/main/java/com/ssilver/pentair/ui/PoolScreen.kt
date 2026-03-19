package com.ssilver.pentair.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import com.ssilver.pentair.ui.theme.PoolBackground
import com.ssilver.pentair.ui.theme.TextDim

@Composable
fun PoolScreen(viewModel: PoolViewModel = hiltViewModel()) {
    val poolSystem by viewModel.state.collectAsState()
    val connectionState by viewModel.connectionState.collectAsState()

    var showSettings by remember { mutableStateOf(false) }
    var showPoolSetpoint by remember { mutableStateOf(false) }
    var showSpaSetpoint by remember { mutableStateOf(false) }

    val system = poolSystem
    val pool = system?.pool
    val spa = system?.spa
    val lights = system?.lights

    // Derive spa state from data
    val spaState = when {
        spa == null -> "off"
        !spa.on -> "off"
        spa.accessories["jets"] == true -> "jets"
        else -> "spa"
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PoolBackground),
    ) {
        // Gear icon - top right
        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(16.dp)
                .size(32.dp)
                .clip(CircleShape)
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                ) { showSettings = true },
        ) {
            Text(
                text = "\u2699",
                fontSize = 18.sp,
                color = TextDim,
            )
        }

        // Pool visual - left aligned, below header area
        PoolVisualCanvas(
            pool = pool,
            spa = spa,
            lights = lights,
            spaState = spaState,
            onPoolSetpointClick = { showPoolSetpoint = true },
            onSpaSetpointClick = { showSpaSetpoint = true },
            onSpaStateChange = { viewModel.setSpaState(it) },
            onLightModeSelect = { viewModel.setLightMode(it) },
            modifier = Modifier
                .align(Alignment.TopStart)
                .padding(start = 8.dp, top = 56.dp),
        )
    }

    // Settings bottom sheet
    if (showSettings && system != null) {
        SettingsDrawer(
            auxiliaries = system.auxiliaries,
            system = system.system,
            pump = system.pump,
            connectionState = connectionState,
            onAuxToggle = { id, on -> viewModel.toggleAux(id, on) },
            onDismiss = { showSettings = false },
        )
    }

    // Pool setpoint bottom sheet
    if (showPoolSetpoint && pool != null) {
        SetpointBottomSheet(
            title = "Pool Temperature",
            currentSetpoint = pool.setpoint,
            onSet = { temp ->
                viewModel.setSetpoint("pool", temp)
                showPoolSetpoint = false
            },
            onDismiss = { showPoolSetpoint = false },
        )
    }

    // Spa setpoint bottom sheet
    if (showSpaSetpoint && spa != null) {
        SetpointBottomSheet(
            title = "Spa Temperature",
            currentSetpoint = spa.setpoint,
            onSet = { temp ->
                viewModel.setSetpoint("spa", temp)
                showSpaSetpoint = false
            },
            onDismiss = { showSpaSetpoint = false },
        )
    }
}
