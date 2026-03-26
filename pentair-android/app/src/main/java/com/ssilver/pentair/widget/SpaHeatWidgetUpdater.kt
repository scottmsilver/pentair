package com.ssilver.pentair.widget

import android.content.Context
import androidx.datastore.preferences.core.MutablePreferences
import androidx.glance.appwidget.GlanceAppWidgetManager
import androidx.glance.appwidget.state.updateAppWidgetState
import com.ssilver.pentair.data.PoolSystem
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Updates all SpaHeatWidget instances whenever pool state changes.
 * Called from PoolViewModel on each state emission.
 */
@Singleton
class SpaHeatWidgetUpdater @Inject constructor(
    @ApplicationContext private val context: Context,
) {
    private val widget = SpaHeatWidget()

    suspend fun update(system: PoolSystem) {
        val manager = GlanceAppWidgetManager(context)
        val glanceIds = manager.getGlanceIds(SpaHeatWidget::class.java)
        if (glanceIds.isEmpty()) return

        for (glanceId in glanceIds) {
            updateAppWidgetState(context, glanceId) { prefs ->
                writeState(prefs, system)
            }
            widget.update(context, glanceId)
        }
    }

    private fun writeState(prefs: MutablePreferences, system: PoolSystem) {
        val spa = system.spa
        val pool = system.pool
        val progress = spa?.spa_heat_progress

        // Spa heat progress fields
        prefs[SpaHeatWidget.ACTIVE] = progress?.active ?: false
        prefs[SpaHeatWidget.PHASE] = progress?.phase ?: "off"
        prefs[SpaHeatWidget.CURRENT_TEMP] = progress?.current_temp_f ?: (spa?.temperature ?: 0)
        prefs[SpaHeatWidget.TARGET_TEMP] = progress?.target_temp_f ?: (spa?.setpoint ?: 0)
        prefs[SpaHeatWidget.PROGRESS_PCT] = progress?.progress_pct ?: 0

        if (progress?.minutes_remaining != null) {
            prefs[SpaHeatWidget.HAS_MINUTES_REMAINING] = true
            prefs[SpaHeatWidget.MINUTES_REMAINING] = progress.minutes_remaining
        } else {
            prefs[SpaHeatWidget.HAS_MINUTES_REMAINING] = false
            prefs.remove(SpaHeatWidget.MINUTES_REMAINING)
        }

        // Idle-state summary fields
        prefs[SpaHeatWidget.POOL_TEMP] = pool?.temperature ?: 0
        prefs[SpaHeatWidget.SPA_TEMP] = spa?.temperature ?: 0
        prefs[SpaHeatWidget.SPA_HEAT_MODE] = spa?.heat_mode ?: "off"
        prefs[SpaHeatWidget.POOL_HEAT_MODE] = pool?.heat_mode ?: "off"
    }
}
