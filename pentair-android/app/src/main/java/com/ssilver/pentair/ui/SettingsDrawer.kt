package com.ssilver.pentair.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ssilver.pentair.data.AuxState
import com.ssilver.pentair.data.BodyState
import com.ssilver.pentair.data.ConnectionState
import com.ssilver.pentair.data.DiagnosticEvent
import com.ssilver.pentair.data.PumpInfo
import com.ssilver.pentair.data.SystemInfo
import com.ssilver.pentair.ui.theme.Accent
import com.ssilver.pentair.ui.theme.TextBright
import com.ssilver.pentair.ui.theme.TextDim
import com.ssilver.pentair.ui.theme.TextFaint
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsDrawer(
    auxiliaries: List<AuxState>,
    system: SystemInfo?,
    pool: BodyState?,
    pump: PumpInfo?,
    connectionState: ConnectionState,
    manualAddress: String,
    discoveredAddress: String?,
    activeAddress: String?,
    isTestingAddress: Boolean,
    diagnostics: List<DiagnosticEvent>,
    useClassicUi: Boolean,
    matter: com.ssilver.pentair.data.MatterStatus?,
    onManualAddressChange: (String) -> Unit,
    onApplyManualAddress: () -> Unit,
    onUseDiscoveredAddress: () -> Unit,
    onTestConnection: () -> Unit,
    onUseClassicUiChange: (Boolean) -> Unit,
    onPoolCircuitChange: (Boolean) -> Unit,
    onAuxToggle: (String, Boolean) -> Unit,
    onMatterRecommission: () -> Unit,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
    ) {
        Column(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp)
                .padding(bottom = 24.dp),
        ) {
            SectionTitle("Daemon")

            OutlinedTextField(
                value = manualAddress,
                onValueChange = onManualAddressChange,
                singleLine = true,
                label = { Text("Address") },
                placeholder = { Text("http://pool-daemon.local:8080") },
                colors = drawerFieldColors(),
                modifier = Modifier.fillMaxWidth(),
            )

            Row(
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Button(
                    onClick = onTestConnection,
                    modifier = Modifier.weight(1f),
                ) {
                    Text(if (isTestingAddress) "Testing..." else "Test Connection")
                }

                Button(
                    onClick = onApplyManualAddress,
                    enabled = manualAddress.isNotBlank(),
                    modifier = Modifier.weight(1f),
                ) {
                    Text("Use This Address")
                }
            }

            if (discoveredAddress != null && discoveredAddress != activeAddress) {
                TextButton(
                    onClick = onUseDiscoveredAddress,
                    modifier = Modifier.align(Alignment.Start),
                ) {
                    Text("Use Discovered Address")
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

            SectionTitle("Diagnostics")
            TechRow("State", connectionStateLabel(connectionState))
            TechRow("Active", activeAddress ?: "None")
            TechRow("Discovered", discoveredAddress ?: "None")

            if (isTestingAddress) {
                TechRow("Probe", "Testing")
            }

            diagnostics.takeLast(8).asReversed().forEach { event ->
                DiagnosticRow(event)
            }

            if (system != null) {
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                SectionTitle("System")
                TechRow("Air", "${system.air_temperature}°")
                TechRow("Controller", system.controller)
                TechRow("Freeze Protection", if (system.freeze_protection) "On" else "Off")
                system.firmware?.takeIf { it.isNotBlank() }?.let { firmware ->
                    TechRow("Firmware", firmware)
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            SectionTitle("Advanced")
            if (pool != null) {
                ToggleRow(
                    title = "Pool Circuit",
                    checked = pool.on,
                    onCheckedChange = onPoolCircuitChange,
                )
            }
            Text(
                text = "Most people should leave the pool circuit alone. Normal control is setpoint, spa mode, and lights.",
                fontSize = 12.sp,
                color = TextFaint,
            )

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            SectionTitle("Google Home")

            TechRow("Matter Pairing", matter?.statusDisplay ?: "Unknown")

            var showConfirm by remember { mutableStateOf(false) }
            var resetSent by remember { mutableStateOf(false) }

            if (matter?.canReset == true) {
                if (!showConfirm && !resetSent) {
                    Button(
                        onClick = { showConfirm = true },
                        modifier = Modifier.fillMaxWidth(),
                        colors = androidx.compose.material3.ButtonDefaults.buttonColors(
                            containerColor = Color(0x26F87171),
                            contentColor = Color(0xFFF87171),
                        ),
                    ) {
                        Text("Reset Matter Pairing")
                    }
                } else if (showConfirm) {
                    Text(
                        text = "This will remove all Google Home devices. You'll need to re-scan the QR code.",
                        fontSize = 13.sp,
                        color = Color(0xFFF87171),
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Button(
                            onClick = {
                                showConfirm = false
                                resetSent = true
                                onMatterRecommission()
                            },
                            colors = androidx.compose.material3.ButtonDefaults.buttonColors(
                                containerColor = Color(0x40F87171),
                                contentColor = Color(0xFFF87171),
                            ),
                            modifier = Modifier.weight(1f),
                        ) { Text("Yes, Reset") }
                        Button(
                            onClick = { showConfirm = false },
                            modifier = Modifier.weight(1f),
                        ) { Text("Cancel") }
                    }
                } else if (resetSent) {
                    Text(
                        text = "Reset sent. In Google Home: + > New device > Matter-enabled device. Manual code: 3497-0112-332",
                        fontSize = 13.sp,
                        color = Accent,
                    )
                }
            } else if (matter != null && !matter.canReset) {
                if (matter.pairingCode != null) {
                    val clipboardManager = androidx.compose.ui.platform.LocalClipboardManager.current
                    var copied by remember { mutableStateOf(false) }
                    val context = androidx.compose.ui.platform.LocalContext.current
                    Text(
                        text = "Ready to pair. In Google Home: + > New device > Matter-enabled device.",
                        fontSize = 13.sp,
                        color = TextDim,
                    )
                    if (activeAddress != null) {
                        TextButton(
                            onClick = {
                                val intent = android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse("$activeAddress/matter"))
                                context.startActivity(intent)
                            },
                            modifier = Modifier.align(Alignment.Start),
                        ) {
                            Text("Scan QR code at $activeAddress/matter")
                        }
                    }
                    Spacer(modifier = Modifier.size(4.dp))
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            text = matter.pairingCode,
                            fontSize = 20.sp,
                            fontWeight = FontWeight.Bold,
                            color = Accent,
                            modifier = Modifier.weight(1f),
                        )
                        Button(
                            onClick = {
                                clipboardManager.setText(androidx.compose.ui.text.AnnotatedString(matter.pairingCode))
                                copied = true
                            },
                            colors = androidx.compose.material3.ButtonDefaults.buttonColors(
                                containerColor = if (copied) Color(0x4038BDF8) else Accent,
                            ),
                        ) {
                            Text(if (copied) "Copied" else "Copy")
                        }
                    }
                } else {
                    Text(
                        text = "Not paired yet.",
                        fontSize = 13.sp,
                        color = TextDim,
                    )
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            SectionTitle("Auxiliaries")
            if (auxiliaries.isEmpty()) {
                Text(
                    text = "No auxiliary circuits are exposed by the daemon.",
                    fontSize = 13.sp,
                    color = TextDim,
                )
            } else {
                auxiliaries.forEach { aux ->
                    ToggleRow(
                        title = aux.name,
                        subtitle = aux.id,
                        checked = aux.on,
                        onCheckedChange = { onAuxToggle(aux.id, it) },
                    )
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            SectionTitle("Pump")
            if (pump != null) {
                TechRow("Type", pump.pump_type)
                TechRow("Status", if (pump.running) "Running" else "Stopped")
                TechRow("RPM", "${pump.rpm}")
                TechRow("Watts", "${pump.watts}")
                TechRow("Flow", "${pump.gpm} gpm")
            } else {
                Text(
                    text = "Waiting for pump telemetry.",
                    fontSize = 13.sp,
                    color = TextDim,
                )
            }
            system?.let {
                TechRow("Temperature Units", if (it.temp_unit == "c") "Celsius" else "Fahrenheit")
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            SectionTitle("Interface")
            SingleChoiceSegmentedButtonRow(
                modifier = Modifier.fillMaxWidth(),
            ) {
                SegmentedButton(
                    selected = !useClassicUi,
                    onClick = { onUseClassicUiChange(false) },
                    shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                    label = { Text("Modern") },
                )
                SegmentedButton(
                    selected = useClassicUi,
                    onClick = { onUseClassicUiChange(true) },
                    shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                    label = { Text("Classic") },
                )
            }
        }
    }
}

@Composable
private fun SectionTitle(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleSmall,
        fontWeight = FontWeight.Medium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun ToggleRow(
    title: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    subtitle: String? = null,
) {
    ListItem(
        headlineContent = {
            Text(
                text = title,
                style = MaterialTheme.typography.bodyLarge,
            )
        },
        supportingContent = subtitle?.let {
            {
                Text(
                    text = it,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        },
        trailingContent = {
            Switch(
                checked = checked,
                onCheckedChange = onCheckedChange,
            )
        },
        colors = ListItemDefaults.colors(
            containerColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun DiagnosticRow(event: DiagnosticEvent) {
    ListItem(
        headlineContent = {
            Text(
                text = event.message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        supportingContent = {
            Text(
                text = event.category.uppercase(),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        trailingContent = {
            Text(
                text = formatDiagnosticTime(event.timestampMillis),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        colors = ListItemDefaults.colors(
            containerColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun TechRow(label: String, value: String) {
    ListItem(
        headlineContent = {
            Text(
                text = label,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        trailingContent = {
            Text(
                text = value,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.SemiBold,
            )
        },
        colors = ListItemDefaults.colors(
            containerColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun drawerFieldColors() = OutlinedTextFieldDefaults.colors(
    focusedTextColor = TextBright,
    unfocusedTextColor = TextBright,
    focusedBorderColor = Accent,
    unfocusedBorderColor = Color.White.copy(alpha = 0.18f),
    focusedLabelColor = Accent,
    unfocusedLabelColor = TextDim,
    focusedPlaceholderColor = TextFaint,
    unfocusedPlaceholderColor = TextFaint,
    cursorColor = Accent,
    focusedContainerColor = Color.Transparent,
    unfocusedContainerColor = Color.Transparent,
)

private fun connectionStateLabel(connectionState: ConnectionState): String = when (connectionState) {
    ConnectionState.CONNECTED -> "Connected"
    ConnectionState.CONNECTING -> "Connecting"
    ConnectionState.DISCONNECTED -> "Disconnected"
    ConnectionState.DISCOVERING -> "Searching"
}

private fun formatDiagnosticTime(timestampMillis: Long): String {
    val formatter = SimpleDateFormat("h:mm:ss a", Locale.US)
    return formatter.format(Date(timestampMillis))
}
