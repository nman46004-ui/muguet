package com.muguet

import android.app.Application
import android.util.Log

class MuguetApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        Log.d("Muguet", "Application created")
        
        // Load native library
        System.loadLibrary("muguet-android")
    }
}