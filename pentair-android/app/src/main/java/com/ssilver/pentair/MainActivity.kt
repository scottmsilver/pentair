package com.ssilver.pentair

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import com.ssilver.pentair.ui.PoolScreen
import com.ssilver.pentair.ui.theme.PoolTheme
import dagger.hilt.android.AndroidEntryPoint

@AndroidEntryPoint
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            PoolTheme {
                PoolScreen()
            }
        }
    }
}
