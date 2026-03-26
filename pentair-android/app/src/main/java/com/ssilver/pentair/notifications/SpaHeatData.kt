package com.ssilver.pentair.notifications

data class SpaHeatData(
    val currentTempF: Int,
    val targetTempF: Int,
    val startTempF: Int,
    val progressPct: Int,
    val minutesRemaining: Int?,
    val phase: String?,
    val milestone: String?,
    val sessionId: String?
)
