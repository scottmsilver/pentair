package com.ssilver.pentair.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ssilver.pentair.data.AuxState
import com.ssilver.pentair.data.ConnectionState
import com.ssilver.pentair.data.PumpInfo
import com.ssilver.pentair.data.SystemInfo
import com.ssilver.pentair.ui.theme.Accent
import com.ssilver.pentair.ui.theme.TextBright
import com.ssilver.pentair.ui.theme.TextDim
import com.ssilver.pentair.ui.theme.TextFaint

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsDrawer(
    auxiliaries: List<AuxState>,
    system: SystemInfo?,
    pump: PumpInfo?,
    connectionState: ConnectionState,
    onAuxToggle: (String, Boolean) -> Unit,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = Color(0xFF1E293B),
        dragHandle = {
            Box(
                modifier = Modifier
                    .padding(top = 8.dp, bottom = 12.dp)
                    .size(width = 36.dp, height = 4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(Color.White.copy(alpha = 0.2f)),
            )
        },
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .padding(bottom = 20.dp),
        ) {
            // Auxiliaries
            if (auxiliaries.isNotEmpty()) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(bottom = 10.dp),
                ) {
                    auxiliaries.forEach { aux ->
                        val isOn = aux.on
                        Box(
                            contentAlignment = Alignment.Center,
                            modifier = Modifier
                                .weight(1f)
                                .clip(RoundedCornerShape(16.dp))
                                .background(
                                    if (isOn) Accent.copy(alpha = 0.12f)
                                    else Color.White.copy(alpha = 0.04f)
                                )
                                .border(
                                    width = 1.dp,
                                    color = if (isOn) Accent.copy(alpha = 0.3f)
                                    else Color.White.copy(alpha = 0.06f),
                                    shape = RoundedCornerShape(16.dp),
                                )
                                .clickable(
                                    interactionSource = remember { MutableInteractionSource() },
                                    indication = null,
                                ) { onAuxToggle(aux.id, !aux.on) }
                                .padding(vertical = 14.dp, horizontal = 6.dp),
                        ) {
                            Text(
                                text = aux.name,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Medium,
                                color = if (isOn) Accent else TextDim,
                            )
                        }
                    }
                }

                HorizontalDivider(
                    color = Color.White.copy(alpha = 0.06f),
                    modifier = Modifier.padding(bottom = 8.dp),
                )
            }

            // Tech info rows
            TechRow(
                label = "Status",
                value = when (connectionState) {
                    ConnectionState.CONNECTED -> "Connected"
                    ConnectionState.DISCONNECTED -> "Disconnected"
                    ConnectionState.DISCOVERING -> "Discovering\u2026"
                },
            )

            if (system != null) {
                TechRow(label = "Equipment Pad", value = "${system.air_temperature}\u00B0F")
                TechRow(label = "Controller", value = system.controller)
                TechRow(label = "Firmware", value = system.firmware ?: "--")
            }

            if (pump != null) {
                TechRow(
                    label = "Pump",
                    value = pump.pump_type + if (pump.running) " (running)" else " (off)",
                )
                if (pump.running) {
                    TechRow(label = "RPM", value = "${pump.rpm} RPM")
                    TechRow(label = "Power", value = "${pump.watts}W")
                    TechRow(label = "Flow", value = "${pump.gpm} GPM")
                } else {
                    TechRow(label = "RPM", value = "\u2014")
                    TechRow(label = "Power", value = "\u2014")
                    TechRow(label = "Flow", value = "\u2014")
                }
            }
        }
    }
}

@Composable
private fun TechRow(label: String, value: String) {
    Row(
        horizontalArrangement = Arrangement.SpaceBetween,
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp),
    ) {
        Text(
            text = label,
            fontSize = 12.sp,
            color = TextFaint,
        )
        Text(
            text = value,
            fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold,
            color = TextDim,
        )
    }
}
