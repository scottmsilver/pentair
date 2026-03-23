package com.ssilver.pentair.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ssilver.pentair.ui.theme.Teal
import com.ssilver.pentair.ui.theme.TextFaint

@Composable
fun SpaSegmentedControl(
    currentState: String,
    onStateChange: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val segments = listOf("off" to "Off", "spa" to "Spa", "jets" to "Jets")
    val trackShape = RoundedCornerShape(10.dp)
    val pillShape = RoundedCornerShape(8.dp)

    Row(
        modifier = modifier
            .clip(trackShape)
            .background(Color.White.copy(alpha = 0.06f))
            .padding(4.dp),
    ) {
        segments.forEach { (key, label) ->
            val isActive = currentState == key
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .weight(1f)
                    .clip(pillShape)
                    .background(
                        if (isActive) Teal.copy(alpha = 0.3f) else Color.Transparent
                    )
                    .clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                    ) { onStateChange(key) }
                    .padding(vertical = 10.dp),
            ) {
                Text(
                    text = label,
                    fontSize = 13.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = if (isActive) Teal else TextFaint,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}
