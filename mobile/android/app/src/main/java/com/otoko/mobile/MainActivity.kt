package com.otoko.mobile

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import com.otoko.mobile.ui.OtokoApp

class MainActivity : ComponentActivity() {
    private val vm: MainViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.BLACK))
        super.onCreate(savedInstanceState)
        // Pour les commandes de lecture dans les notifications
        if (Build.VERSION.SDK_INT >= 33) requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        // Android 9 et avant : écrire dans Musique demande la permission de stockage
        if (Build.VERSION.SDK_INT < 29) requestPermissions(arrayOf(Manifest.permission.WRITE_EXTERNAL_STORAGE), 2)
        readLink(intent)
        setContent { OtokoApp(vm) }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        readLink(intent)
    }

    /** « Ouvrir l'app » depuis la page mobile : l'adresse est proposée, jamais adoptée sans qu'on touche « Se connecter » */
    private fun readLink(i: Intent?) {
        val link = i?.data ?: return
        if (link.scheme == "otokovault" && link.host == "relier") link.getQueryParameter("adresse")?.let { vm.suggestAddress(it) }
    }
}
