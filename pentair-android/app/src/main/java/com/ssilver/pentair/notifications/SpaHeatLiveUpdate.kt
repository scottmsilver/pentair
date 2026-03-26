package com.ssilver.pentair.notifications

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.core.app.NotificationCompat
import com.ssilver.pentair.MainActivity
import com.ssilver.pentair.R
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class SpaHeatLiveUpdate @Inject constructor(@ApplicationContext private val context: Context) {

    companion object {
        private const val CHANNEL_ID = "spa_heat_live"
        private const val CHANNEL_NAME = "Spa Heating"
        private const val NOTIFICATION_ID = 9001
        private const val AUTO_DISMISS_DELAY_MS = 30_000L

        // Warming color palette
        private const val COLOR_DEEP_ORANGE = 0xFFFF6D00.toInt()
        private const val COLOR_ORANGE = 0xFFFF9100.toInt()
        private const val COLOR_AMBER = 0xFFFFC107.toInt()
        private const val COLOR_GREEN = 0xFF4CAF50.toInt()
        private const val COLOR_GRAY = 0xFF424242.toInt()

        // Segment IDs for smooth animation between updates
        private const val SEGMENT_FILLED = 1
        private const val SEGMENT_REMAINING = 2
    }

    private val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    private val handler = Handler(Looper.getMainLooper())
    @Volatile
    private var channelCreated = false

    fun update(data: SpaHeatData) {
        if (data.phase == "reached") {
            end(data.currentTempF)
            return
        }
        ensureChannel()
        if (Build.VERSION.SDK_INT >= 36) {
            postProgressStyle(data)
        } else {
            postFallbackNotification(data)
        }
    }

    fun end(finalTemp: Int) {
        ensureChannel()
        handler.removeCallbacksAndMessages(null)

        if (Build.VERSION.SDK_INT >= 36) {
            postReachedProgressStyle(finalTemp)
        } else {
            postReachedFallback(finalTemp)
        }

        handler.postDelayed({ manager.cancel(NOTIFICATION_ID) }, AUTO_DISMISS_DELAY_MS)
    }

    private fun ensureChannel() {
        if (channelCreated) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            CHANNEL_NAME,
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "Live spa heating progress"
        }
        manager.createNotificationChannel(channel)
        channelCreated = true
    }

    private fun tapIntent(): PendingIntent {
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        return PendingIntent.getActivity(
            context, 0, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
    }

    private fun progressColor(pct: Int): Int = when {
        pct < 30 -> COLOR_DEEP_ORANGE
        pct < 70 -> COLOR_ORANGE
        pct < 90 -> COLOR_AMBER
        else -> COLOR_GREEN
    }

    private fun shortEta(minutes: Int?): String? = when {
        minutes == null -> null
        minutes < 60 -> "~${minutes}m"
        else -> "~${minutes / 60}h${minutes % 60}m"
    }

    private fun etaText(minutes: Int?): String = when {
        minutes == null -> "Heating..."
        minutes < 60 -> "~$minutes min"
        else -> {
            val h = minutes / 60
            val m = minutes % 60
            if (m == 0) "~${h} hr" else "~${h} hr $m min"
        }
    }

    // --- API 36+ ProgressStyle (Full Live Update) ---

    private fun postProgressStyle(data: SpaHeatData) {
        if (Build.VERSION.SDK_INT < 36) return

        val pct = data.progressPct.coerceIn(0, 100)
        val isEstimating = data.phase == "started" || data.minutesRemaining == null

        val builder = Notification.Builder(context, CHANNEL_ID)
            .setSmallIcon(Icon.createWithResource(context, R.drawable.ic_flame))
            .setContentTitle("Spa Heating     ${etaText(data.minutesRemaining)}")
            .setContentText("${data.currentTempF}\u00B0F  \u2192  ${data.targetTempF}\u00B0F")
            .setSubText("Pentair")
            .setColor(COLOR_ORANGE)
            .setOngoing(true)
            .setContentIntent(tapIntent())
            .setShowWhen(false)
            .setOnlyAlertOnce(true)

        // Request Live Update promotion (status bar chip, top of shade, lock screen)
        try {
            // EXTRA_REQUEST_PROMOTED_ONGOING may be in API 36.1+
            val field = Notification::class.java.getField("EXTRA_REQUEST_PROMOTED_ONGOING")
            builder.addExtras(Bundle().apply {
                putBoolean(field.get(null) as String, true)
            })
        } catch (_: Exception) {
            // Fallback: use the known string constant directly
            builder.addExtras(Bundle().apply {
                putBoolean("android.requestPromotedOngoing", true)
            })
        }

        // Status bar chip text: show ETA or current temp
        val chipText = shortEta(data.minutesRemaining) ?: "${data.currentTempF}\u00B0"
        builder.setShortCriticalText(chipText)

        // Build ProgressStyle
        val style = Notification.ProgressStyle()
            .setStyledByProgress(false)  // We control segment colors ourselves

        if (isEstimating) {
            // Indeterminate animation while waiting for ETA
            style.setProgressIndeterminate(true)
            // Single segment colors the indeterminate bar
            style.setProgressSegments(listOf(
                Notification.ProgressStyle.Segment(100)
                    .setColor(COLOR_DEEP_ORANGE)
                    .setId(SEGMENT_FILLED)
            ))
        } else {
            // Determinate progress with warming colors
            val filledColor = progressColor(pct)
            val segments = when {
                pct == 0 -> listOf(
                    Notification.ProgressStyle.Segment(100)
                        .setColor(COLOR_GRAY)
                        .setId(SEGMENT_REMAINING)
                )
                pct >= 100 -> listOf(
                    Notification.ProgressStyle.Segment(100)
                        .setColor(filledColor)
                        .setId(SEGMENT_FILLED)
                )
                else -> listOf(
                    Notification.ProgressStyle.Segment(pct)
                        .setColor(filledColor)
                        .setId(SEGMENT_FILLED),
                    Notification.ProgressStyle.Segment(100 - pct)
                        .setColor(COLOR_GRAY)
                        .setId(SEGMENT_REMAINING)
                )
            }
            style.setProgress(pct)
            style.setProgressSegments(segments)

            // Flame tracker icon that slides along the progress bar
            style.setProgressTrackerIcon(
                Icon.createWithResource(context, R.drawable.ic_flame_tracker)
            )
        }

        builder.setStyle(style)

        // Milestone events buzz; silent progress updates do not
        if (data.milestone != null && data.milestone != "estimate_ready") {
            builder.setDefaults(Notification.DEFAULT_SOUND)
        } else {
            builder.setDefaults(0)
        }

        manager.notify(NOTIFICATION_ID, builder.build())
    }

    private fun postReachedProgressStyle(finalTemp: Int) {
        if (Build.VERSION.SDK_INT < 36) return

        val builder = Notification.Builder(context, CHANNEL_ID)
            .setSmallIcon(Icon.createWithResource(context, R.drawable.ic_check_circle))
            .setContentTitle("Spa is ready!")
            .setContentText("${finalTemp}\u00B0F \u2014 enjoy your soak")
            .setSubText("Pentair")
            .setColor(COLOR_GREEN)
            .setOngoing(false)
            .setContentIntent(tapIntent())
            .setShowWhen(false)
            .setDefaults(Notification.DEFAULT_SOUND or Notification.DEFAULT_VIBRATE)
            .setShortCriticalText("\u2713")

        val style = Notification.ProgressStyle()
            .setStyledByProgress(false)
            .setProgress(100)
            .setProgressSegments(listOf(
                Notification.ProgressStyle.Segment(100)
                    .setColor(COLOR_GREEN)
                    .setId(SEGMENT_FILLED)
            ))
        builder.setStyle(style)

        manager.notify(NOTIFICATION_ID, builder.build())
    }

    // --- Fallback (pre-API 36) ---

    private fun postFallbackNotification(data: SpaHeatData) {
        val isEstimating = data.phase == "started" || data.minutesRemaining == null

        val builder = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_flame)
            .setContentTitle("Spa Heating     ${etaText(data.minutesRemaining)}")
            .setContentText("${data.currentTempF}\u00B0F  \u2192  ${data.targetTempF}\u00B0F     ${data.progressPct}%")
            .setProgress(100, data.progressPct, isEstimating)
            .setColor(COLOR_ORANGE)
            .setOngoing(true)
            .setContentIntent(tapIntent())
            .setShowWhen(false)

        // Milestone events buzz; silent progress updates do not
        if (data.milestone != null && data.milestone != "estimate_ready") {
            builder.setPriority(NotificationCompat.PRIORITY_HIGH)
            builder.setDefaults(NotificationCompat.DEFAULT_SOUND)
        } else {
            builder.setPriority(NotificationCompat.PRIORITY_LOW)
            builder.setDefaults(0)
            builder.setSilent(true)
            builder.setOnlyAlertOnce(true)
        }

        manager.notify(NOTIFICATION_ID, builder.build())
    }

    private fun postReachedFallback(finalTemp: Int) {
        val builder = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_check_circle)
            .setContentTitle("Spa is ready!")
            .setContentText("${finalTemp}\u00B0F \u2014 enjoy your soak")
            .setProgress(100, 100, false)
            .setColor(COLOR_GREEN)
            .setOngoing(false)
            .setContentIntent(tapIntent())
            .setShowWhen(false)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setDefaults(NotificationCompat.DEFAULT_SOUND or NotificationCompat.DEFAULT_VIBRATE)

        manager.notify(NOTIFICATION_ID, builder.build())
    }
}
