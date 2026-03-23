package com.ssilver.pentair.data

import com.squareup.moshi.JsonClass

@JsonClass(generateAdapter = true)
data class PoolSystem(
    val pool: BodyState?,
    val spa: SpaState?,
    val lights: LightState?,
    val auxiliaries: List<AuxState>,
    val pump: PumpInfo?,
    val system: SystemInfo
)

@JsonClass(generateAdapter = true)
data class BodyState(
    val on: Boolean,
    val active: Boolean = false,
    val temperature: Int,
    val setpoint: Int,
    val heat_mode: String,
    val heating: String
)

@JsonClass(generateAdapter = true)
data class SpaState(
    val on: Boolean,
    val active: Boolean = false,
    val temperature: Int,
    val setpoint: Int,
    val heat_mode: String,
    val heating: String,
    val accessories: Map<String, Boolean>
)

@JsonClass(generateAdapter = true)
data class LightState(
    val on: Boolean,
    val mode: String?,
    val available_modes: List<String>
)

@JsonClass(generateAdapter = true)
data class AuxState(
    val id: String,
    val name: String,
    val on: Boolean
)

@JsonClass(generateAdapter = true)
data class PumpInfo(
    val pump_type: String,
    val running: Boolean,
    val watts: Int,
    val rpm: Int,
    val gpm: Int
)

@JsonClass(generateAdapter = true)
data class SystemInfo(
    val controller: String,
    val firmware: String?,
    val temp_unit: String,
    val air_temperature: Int,
    val freeze_protection: Boolean,
    val pool_spa_shared_pump: Boolean
)

@JsonClass(generateAdapter = true)
data class ApiResponse(
    val ok: Boolean,
    val error: String? = null
)
