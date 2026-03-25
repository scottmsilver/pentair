package com.ssilver.pentair.di

import com.ssilver.pentair.data.DeviceRegistrationClient
import com.ssilver.pentair.data.FirebaseMessagingTokenProvider
import com.ssilver.pentair.data.MessagingTokenProvider
import com.ssilver.pentair.data.OkHttpDeviceRegistrationClient
import dagger.Binds
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
abstract class MessagingModule {
    @Binds
    @Singleton
    abstract fun bindDeviceRegistrationClient(
        impl: OkHttpDeviceRegistrationClient,
    ): DeviceRegistrationClient

    @Binds
    @Singleton
    abstract fun bindMessagingTokenProvider(
        impl: FirebaseMessagingTokenProvider,
    ): MessagingTokenProvider
}
