package com.ssilver.pentair.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
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
    val poolSystem by viewModel.state.collectAsStateWithLifecycle()
    val connectionState by viewModel.connectionState.collectAsStateWithLifecycle()
    val snackbarHostState = remember { SnackbarHostState() }

    LaunchedEffect(Unit) {
        viewModel.rejections.collect { message ->
            snackbarHostState.showSnackbar(message)
        }
    }

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
        // Pool visual - fills screen width
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
                .padding(start = 10.dp, end = 10.dp, top = 44.dp),
        )

        // Gear icon — top right, outside the pool visual (matches web UI)
        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(top = 40.dp, end = 8.dp)
                .size(48.dp)
                .clip(CircleShape)
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                ) { showSettings = true },
        ) {
            Text(
                text = "\u2699",
                fontSize = 22.sp,
                color = TextDim,
            )
        }

        SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(bottom = 16.dp),
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
