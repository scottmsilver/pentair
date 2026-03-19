package com.ssilver.pentair.data

import retrofit2.http.Body
import retrofit2.http.GET
import retrofit2.http.POST
import retrofit2.http.Path

interface PoolApiClient {
    @GET("/api/pool")
    suspend fun getPool(): PoolSystem

    @POST("/api/spa/on")
    suspend fun spaOn(): ApiResponse

    @POST("/api/spa/on")
    suspend fun spaOnWithSetpoint(@Body body: @JvmSuppressWildcards Map<String, Any>): ApiResponse

    @POST("/api/spa/off")
    suspend fun spaOff(): ApiResponse

    @POST("/api/spa/heat")
    suspend fun spaHeat(@Body body: @JvmSuppressWildcards Map<String, Any>): ApiResponse

    @POST("/api/spa/jets/on")
    suspend fun jetsOn(): ApiResponse

    @POST("/api/spa/jets/off")
    suspend fun jetsOff(): ApiResponse

    @POST("/api/pool/on")
    suspend fun poolOn(): ApiResponse

    @POST("/api/pool/on")
    suspend fun poolOnWithSetpoint(@Body body: @JvmSuppressWildcards Map<String, Any>): ApiResponse

    @POST("/api/pool/off")
    suspend fun poolOff(): ApiResponse

    @POST("/api/pool/heat")
    suspend fun poolHeat(@Body body: @JvmSuppressWildcards Map<String, Any>): ApiResponse

    @POST("/api/lights/on")
    suspend fun lightsOn(): ApiResponse

    @POST("/api/lights/off")
    suspend fun lightsOff(): ApiResponse

    @POST("/api/lights/mode")
    suspend fun lightsMode(@Body body: @JvmSuppressWildcards Map<String, Any>): ApiResponse

    @POST("/api/auxiliary/{id}/on")
    suspend fun auxOn(@Path("id") id: String): ApiResponse

    @POST("/api/auxiliary/{id}/off")
    suspend fun auxOff(@Path("id") id: String): ApiResponse

    @POST("/api/devices/register")
    suspend fun registerDevice(@Body body: Map<String, String>): ApiResponse
}
