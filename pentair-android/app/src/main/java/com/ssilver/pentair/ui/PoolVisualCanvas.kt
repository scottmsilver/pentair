package com.ssilver.pentair.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ssilver.pentair.data.BodyState
import com.ssilver.pentair.data.LightState
import com.ssilver.pentair.data.SpaState
import com.ssilver.pentair.ui.theme.DeckGray
import com.ssilver.pentair.ui.theme.DeckGrayLight
import com.ssilver.pentair.ui.theme.PoolBlue
import com.ssilver.pentair.ui.theme.PoolBlueLight
import com.ssilver.pentair.ui.theme.SpaTeal
import com.ssilver.pentair.ui.theme.SpaTealLight
import com.ssilver.pentair.ui.theme.TextBright
import com.ssilver.pentair.ui.theme.TextFaint
import com.ssilver.pentair.ui.theme.Warm
import com.ssilver.pentair.ui.theme.WaterOff

@Composable
fun PoolVisualCanvas(
    pool: BodyState?,
    spa: SpaState?,
    lights: LightState?,
    spaState: String,
    onPoolSetpointClick: () -> Unit,
    onSpaSetpointClick: () -> Unit,
    onSpaStateChange: (String) -> Unit,
    onLightModeSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val infiniteTransition = rememberInfiniteTransition(label = "shimmer")
    val shimmerAlpha by infiniteTransition.animateFloat(
        initialValue = 0.5f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(5000, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "shimmerAlpha",
    )

    val poolOn = pool?.on == true
    val spaOn = spa?.on == true

    val deckShape = RoundedCornerShape(14.dp)
    val waterShape = RoundedCornerShape(16.dp)
    val spaShape = RoundedCornerShape(16.dp)

    // Deck container — matches web UI: 3px padding, 14px border-radius
    Box(
        modifier = modifier
            .fillMaxWidth()
            .aspectRatio(4f / 5f)
            .clip(deckShape)
            .background(
                Brush.linearGradient(
                    colors = listOf(DeckGrayLight, DeckGray, DeckGrayLight, DeckGray),
                    start = Offset.Zero,
                    end = Offset(Float.POSITIVE_INFINITY, Float.POSITIVE_INFINITY),
                )
            )
            .padding(3.dp),
    ) {
        // Pool water — fills entire deck interior
        Box(
            modifier = Modifier
                .fillMaxSize()
                .clip(waterShape)
                .background(
                    if (poolOn) {
                        Brush.linearGradient(
                            colors = listOf(PoolBlue, PoolBlueLight, PoolBlue),
                            start = Offset(0f, 0f),
                            end = Offset(100f, Float.POSITIVE_INFINITY),
                        )
                    } else {
                        Brush.linearGradient(
                            colors = listOf(WaterOff, WaterOff),
                        )
                    }
                )
                .then(
                    if (poolOn) {
                        Modifier.drawBehind {
                            drawCircle(
                                color = Color.White.copy(alpha = 0.04f * shimmerAlpha),
                                radius = size.width * 0.4f,
                                center = Offset(size.width * 0.2f, size.height * 0.7f),
                            )
                            drawCircle(
                                color = Color.White.copy(alpha = 0.03f * shimmerAlpha),
                                radius = size.width * 0.3f,
                                center = Offset(size.width * 0.65f, size.height * 0.25f),
                            )
                        }
                    } else {
                        Modifier
                    }
                )
                .border(
                    width = 2.dp,
                    color = Color.Black.copy(alpha = 0.3f),
                    shape = waterShape,
                ),
        ) {
            // Pool temperature — bottom left
            if (pool != null) {
                Column(
                    modifier = Modifier
                        .align(Alignment.BottomStart)
                        .padding(start = 20.dp, bottom = 56.dp),
                ) {
                    Text(
                        text = "${pool.temperature}\u00B0",
                        fontSize = 40.sp,
                        fontWeight = FontWeight.Bold,
                        color = if (poolOn) TextBright else TextBright.copy(alpha = 0.5f),
                        letterSpacing = (-2).sp,
                        lineHeight = 40.sp,
                    )
                    Text(
                        text = "set ${pool.setpoint}\u00B0",
                        fontSize = 13.sp,
                        color = if (poolOn) Color.White.copy(alpha = 0.5f) else TextFaint,
                        modifier = Modifier.clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            onClick = onPoolSetpointClick,
                        ),
                    )
                    if (pool.heating != "off") {
                        Text(
                            text = "Heating",
                            fontSize = 11.sp,
                            fontWeight = FontWeight.SemiBold,
                            color = Warm,
                            modifier = Modifier.padding(top = 3.dp),
                        )
                    }
                }
            }

            // Spa area — upper right, 42% width, 55% height (matches web UI)
            Box(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .fillMaxWidth(0.42f)
                    .fillMaxHeight(0.55f)
                    .border(
                        width = 5.dp,
                        color = DeckGray,
                        shape = spaShape,
                    )
                    .clip(spaShape)
                    .background(
                        if (spaOn) {
                            Brush.linearGradient(
                                colors = listOf(SpaTeal, SpaTealLight, SpaTeal),
                                start = Offset(0f, 0f),
                                end = Offset(Float.POSITIVE_INFINITY, Float.POSITIVE_INFINITY),
                            )
                        } else {
                            Brush.linearGradient(
                                colors = listOf(WaterOff, WaterOff),
                            )
                        }
                    )
                    .then(
                        if (spaOn) {
                            Modifier.drawBehind {
                                drawCircle(
                                    color = Color.White.copy(alpha = 0.12f * shimmerAlpha),
                                    radius = size.width * 0.25f,
                                    center = Offset(size.width * 0.3f, size.height * 0.3f),
                                )
                                drawCircle(
                                    color = Color.White.copy(alpha = 0.08f * shimmerAlpha),
                                    radius = size.width * 0.2f,
                                    center = Offset(size.width * 0.7f, size.height * 0.5f),
                                )
                                drawCircle(
                                    color = Color.White.copy(alpha = 0.10f * shimmerAlpha),
                                    radius = size.width * 0.22f,
                                    center = Offset(size.width * 0.5f, size.height * 0.8f),
                                )
                            }
                        } else {
                            Modifier
                        }
                    ),
            ) {
                // Spa temperature — centered
                if (spa != null) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        modifier = Modifier
                            .align(Alignment.Center)
                            .padding(horizontal = 8.dp),
                    ) {
                        Text(
                            text = "${spa.temperature}\u00B0",
                            fontSize = 36.sp,
                            fontWeight = FontWeight.Bold,
                            color = if (spaOn) TextBright else TextBright.copy(alpha = 0.5f),
                            letterSpacing = (-2).sp,
                            lineHeight = 36.sp,
                        )
                        Text(
                            text = "set ${spa.setpoint}\u00B0",
                            fontSize = 12.sp,
                            color = if (spaOn) Color.White.copy(alpha = 0.5f) else TextFaint,
                            modifier = Modifier.clickable(
                                interactionSource = remember { MutableInteractionSource() },
                                indication = null,
                                onClick = onSpaSetpointClick,
                            ),
                        )
                        if (spa.heating != "off") {
                            Text(
                                text = "Heating",
                                fontSize = 11.sp,
                                fontWeight = FontWeight.SemiBold,
                                color = Warm,
                                modifier = Modifier.padding(top = 3.dp),
                            )
                        }
                    }
                }

                // Spa segmented control at bottom
                SpaSegmentedControl(
                    currentState = spaState,
                    onStateChange = onSpaStateChange,
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .padding(6.dp),
                )
            }

            // Light picker — bottom left
            LightPicker(
                lights = lights,
                onModeSelect = onLightModeSelect,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(10.dp),
            )
        }
    }
}
