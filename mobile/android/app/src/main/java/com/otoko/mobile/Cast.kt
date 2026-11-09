package com.otoko.mobile

import android.content.Context
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * Diffusion vers un lecteur réseau (Naim Mu-so) : recherche des appareils, appareil choisi.
 * Le service de lecture observe [target] et bascule son lecteur (téléphone ↔ Mu-so).
 */
class Cast(private val app: App) {
    private val prefs = app.getSharedPreferences("diffusion", Context.MODE_PRIVATE)

    private val _devices = MutableStateFlow<List<UpnpDevice>>(emptyList())
    val devices: StateFlow<List<UpnpDevice>> = _devices
    private val _searching = MutableStateFlow(false)
    val searching: StateFlow<Boolean> = _searching
    private val _target = MutableStateFlow<UpnpDevice?>(null)
    /** Appareil sur lequel on diffuse, null : ce téléphone */
    val target: StateFlow<UpnpDevice?> = _target
    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 4)
    val messages: SharedFlow<String> = _messages

    /** Nom du dernier appareil utilisé (affiché en premier) */
    val lastDeviceUdn: String? get() = prefs.getString("udn", null)

    fun search() {
        if (_searching.value) return
        _searching.value = true
        app.scope.launch {
            val found = runCatching { Upnp.discover(app) }.getOrDefault(emptyList())
            val last = lastDeviceUdn
            _devices.value = found.sortedByDescending { it.udn == last }
            _searching.value = false
        }
    }

    /** null si la diffusion peut démarrer, sinon la raison */
    fun castTo(device: UpnpDevice): String? {
        if (!app.onWifi()) return "Wi-Fi maison requis pour diffuser."
        if (app.settings.lanServer == null)
            return "Ton PC n'est pas joignable sur le Wi-Fi : autorise le Wi-Fi sur ton PC (Mobile et partage), ouvre l'appli une fois en Wi-Fi, puis réessaie."
        prefs.edit().putString("udn", device.udn).putString("location", device.location).apply()
        _target.value = device
        return null
    }

    fun stopCasting() { _target.value = null }

    /** Appelé par le service quand la Mu-so ne répond plus : retour au téléphone, en pause */
    fun lost(name: String) {
        _target.value = null
        message("$name ne répond plus : retour sur le téléphone.")
    }

    fun message(text: String) { _messages.tryEmit(text) }
}
