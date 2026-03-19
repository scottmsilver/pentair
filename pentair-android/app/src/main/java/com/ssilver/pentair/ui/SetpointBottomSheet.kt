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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ssilver.pentair.ui.theme.Accent
import com.ssilver.pentair.ui.theme.PoolBackground
import com.ssilver.pentair.ui.theme.TextBright
import com.ssilver.pentair.ui.theme.TextDim

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SetpointBottomSheet(
    title: String,
    currentSetpoint: Int,
    onSet: (Int) -> Unit,
    onDismiss: () -> Unit,
) {
    var tempValue by remember { mutableIntStateOf(currentSetpoint) }
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = Color(0xFF1E293B),
        dragHandle = {
            Box(
                modifier = Modifier
                    .padding(top = 8.dp, bottom = 20.dp)
                    .size(width = 36.dp, height = 4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(Color.White.copy(alpha = 0.2f)),
            )
        },
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 24.dp)
                .padding(bottom = 20.dp),
        ) {
            // Title
            Text(
                text = title,
                fontSize = 16.sp,
                fontWeight = FontWeight.SemiBold,
                color = TextBright,
            )

            Spacer(Modifier.height(20.dp))

            // Large temperature display
            Row(
                verticalAlignment = Alignment.Bottom,
            ) {
                Text(
                    text = "$tempValue",
                    fontSize = 68.sp,
                    fontWeight = FontWeight.Bold,
                    color = TextBright,
                    letterSpacing = (-3).sp,
                    lineHeight = 68.sp,
                )
                Text(
                    text = "\u00B0",
                    fontSize = 26.sp,
                    fontWeight = FontWeight.Normal,
                    color = TextDim,
                    modifier = Modifier.padding(bottom = 10.dp),
                )
            }

            Spacer(Modifier.height(20.dp))

            // +/- buttons
            Row(
                horizontalArrangement = Arrangement.Center,
            ) {
                // Minus button
                Box(
                    contentAlignment = Alignment.Center,
                    modifier = Modifier
                        .size(56.dp)
                        .clip(CircleShape)
                        .background(Color.White.copy(alpha = 0.08f))
                        .border(1.dp, Color.White.copy(alpha = 0.12f), CircleShape)
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                        ) {
                            tempValue = (tempValue - 1).coerceAtLeast(40)
                        },
                ) {
                    Text(
                        text = "\u2212",
                        fontSize = 26.sp,
                        fontWeight = FontWeight.Light,
                        color = TextBright,
                    )
                }

                Spacer(Modifier.width(28.dp))

                // Plus button
                Box(
                    contentAlignment = Alignment.Center,
                    modifier = Modifier
                        .size(56.dp)
                        .clip(CircleShape)
                        .background(Color.White.copy(alpha = 0.08f))
                        .border(1.dp, Color.White.copy(alpha = 0.12f), CircleShape)
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                        ) {
                            tempValue = (tempValue + 1).coerceAtMost(104)
                        },
                ) {
                    Text(
                        text = "+",
                        fontSize = 26.sp,
                        fontWeight = FontWeight.Light,
                        color = TextBright,
                    )
                }
            }

            Spacer(Modifier.height(24.dp))

            // Cancel / Set buttons
            Row(
                modifier = Modifier.fillMaxWidth(),
            ) {
                // Cancel
                Box(
                    contentAlignment = Alignment.Center,
                    modifier = Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(14.dp))
                        .background(Color.White.copy(alpha = 0.08f))
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            onClick = onDismiss,
                        )
                        .padding(vertical = 15.dp),
                ) {
                    Text(
                        text = "Cancel",
                        fontSize = 15.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = TextBright,
                    )
                }

                Spacer(Modifier.width(10.dp))

                // Set
                Box(
                    contentAlignment = Alignment.Center,
                    modifier = Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(14.dp))
                        .background(Accent)
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                        ) {
                            onSet(tempValue)
                        }
                        .padding(vertical = 15.dp),
                ) {
                    Text(
                        text = "Set",
                        fontSize = 15.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = PoolBackground,
                    )
                }
            }
        }
    }
}
