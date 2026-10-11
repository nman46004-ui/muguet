package com.muguet

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.view.Window
import android.view.WindowManager
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.WindowCompat

class MainActivity : AppCompatActivity() {
    private var touchOverlay: TouchOverlay? = null

    companion object {
        // Native method to pass launch config
        external fun nativeSetLaunchConfig(
            startMode: Int,
            scale: Float,
            fpsLimit: Int,
            touchSens: Float,
            fullscreen: Boolean,
            mute: Boolean,
            multiplayer: Boolean,
            host: Boolean,
            serverAddress: String,
            playerName: String
        )
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        
        // Read launch config from intent
        val intent = intent
        val startMode = intent.getIntExtra("start_mode", 2)
        val scale = intent.getFloatExtra("scale", 0.5f)
        val fpsLimit = intent.getIntExtra("fps_limit", 60)
        val touchSens = intent.getFloatExtra("touch_sens", 2.0f)
        val fullscreen = intent.getBooleanExtra("fullscreen", true)
        val mute = intent.getBooleanExtra("mute", false)
        val multiplayer = intent.getBooleanExtra("multiplayer", false)
        val host = intent.getBooleanExtra("host", true)
        val serverAddress = intent.getStringExtra("server") ?: ""
        val playerName = intent.getStringExtra("player_name") ?: ""
        
        // Pass to native (sets env vars for Rust)
        nativeSetLaunchConfig(
            startMode, scale, fpsLimit, touchSens,
            fullscreen, mute, multiplayer, host,
            serverAddress, playerName
        )
        
        // Full screen, no title bar
        requestWindowFeature(Window.FEATURE_NO_TITLE)
        window.setFlags(
            WindowManager.LayoutParams.FLAG_FULLSCREEN,
            WindowManager.LayoutParams.FLAG_FULLSCREEN
        )
        WindowCompat.setDecorFitsSystemWindows(window, false)
        
        // Keep screen on
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        
        setContentView(android.R.id.content)
        
        // Initialize touch overlay
        touchOverlay = TouchOverlay(this)
        touchOverlay?.initialize(window)
        
        // Add touch overlay to root view
        (android.R.id.content as ViewGroup).addView(touchOverlay!!)
    }

    override fun onResume() {
        super.onResume()
        touchOverlay?.update(false, false)
    }

    override fun onPause() {
        super.onPause()
        touchOverlay?.update(true, false)
    }

    override fun onDestroy() {
        super.onDestroy()
        touchOverlay = null
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            window.decorView.systemUiVisibility = (View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                or View.SYSTEM_UI_FLAG_FULLSCREEN)
        }
    }
}