package com.ssilver.pentair.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
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
    val manualAddress by viewModel.manualAddress.collectAsStateWithLifecycle()
    val discoveredAddress by viewModel.discoveredAddress.collectAsStateWithLifecycle()
    val activeAddress by viewModel.activeAddress.collectAsStateWithLifecycle()
    val isTestingAddress by viewModel.isTestingAddress.collectAsStateWithLifecycle()
    val diagnostics by viewModel.diagnostics.collectAsStateWithLifecycle()
    val snackbarHostState = remember { SnackbarHostState() }

    LaunchedEffect(Unit) {
        viewModel.rejections.collect { message ->
            snackbarHostState.showSnackbar(message)
        }
    }

    var showSettings by remember { mutableStateOf(false) }
    var showPoolSetpoint by remember { mutableStateOf(false) }
    var showSpaSetpoint by remember { mutableStateOf(false) }
    var useClassicUi by rememberSaveable { mutableStateOf(false) }

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
        if (useClassicUi) {
            AnimatedVisibility(
                visible = system == null,
                enter = fadeIn(),
                exit = fadeOut(),
            ) {
                Box(
                    contentAlignment = Alignment.Center,
                    modifier = Modifier.fillMaxSize(),
                ) {
                    CircularProgressIndicator(
                        color = TextDim,
                        strokeWidth = 2.dp,
                        modifier = Modifier.size(32.dp),
                    )
                }
            }

            AnimatedVisibility(
                visible = system != null,
                enter = fadeIn(),
                exit = fadeOut(),
            ) {
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
                        .fillMaxSize()
                        .windowInsetsPadding(WindowInsets.systemBars)
                        .padding(start = 28.dp, end = 28.dp, top = 24.dp, bottom = 20.dp),
                )
            }
        } else {
            PoolModernScreen(
                pool = pool,
                spa = spa,
                lights = lights,
                isLoading = system == null,
                sharedPump = system?.system?.pool_spa_shared_pump == true,
                connectionState = connectionState,
                spaState = spaState,
                onShowSettings = { showSettings = true },
                onPoolSetpointClick = { showPoolSetpoint = true },
                onSpaStateChange = { viewModel.setSpaState(it) },
                onSpaSetpointClick = { showSpaSetpoint = true },
                onLightModeSelect = { viewModel.setLightMode(it) },
                modifier = Modifier
                    .fillMaxSize()
                    .windowInsetsPadding(WindowInsets.systemBars),
            )
        }

        if (system != null && useClassicUi) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .windowInsetsPadding(WindowInsets.statusBars)
                    .padding(top = 16.dp, end = 20.dp)
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
        }

        SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .windowInsetsPadding(WindowInsets.navigationBars)
                .padding(bottom = 16.dp),
        )
    }

    if (showSettings) {
        SettingsDrawer(
            auxiliaries = system?.auxiliaries.orEmpty(),
            system = system?.system,
            pool = system?.pool,
            pump = system?.pump,
            connectionState = connectionState,
            manualAddress = manualAddress,
            discoveredAddress = discoveredAddress,
            activeAddress = activeAddress,
            isTestingAddress = isTestingAddress,
            diagnostics = diagnostics,
            useClassicUi = useClassicUi,
            onManualAddressChange = viewModel::setManualAddress,
            onApplyManualAddress = viewModel::applyManualAddress,
            onUseDiscoveredAddress = viewModel::useDiscoveredAddress,
            onTestConnection = viewModel::testManualAddress,
            onUseClassicUiChange = { useClassicUi = it },
            onPoolCircuitChange = { viewModel.setPoolMode(if (it) "on" else "off") },
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
