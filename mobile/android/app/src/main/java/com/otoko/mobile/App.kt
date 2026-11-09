package com.otoko.mobile

import android.app.Application
import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** Par où passe l'appli pour joindre ton PC */
enum class Connection(val label: String) {
    SEARCHING("Recherche de ton PC…"),
    LAN("Wi-Fi maison (direct)"),
    TAILSCALE("Tailscale"),
    OFFLINE("Hors ligne"),
}

class App : Application() {
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    lateinit var settings: Settings
    lateinit var api: Api
    lateinit var downloads: Downloads
    lateinit var cast: Cast

    private val _connection = MutableStateFlow(Connection.SEARCHING)
    val connection: StateFlow<Connection> = _connection

    override fun onCreate() {
        super.onCreate()
        instance = this
        settings = Settings(this)
        api = Api { settings.server.value }
        downloads = Downloads(this, api, settings, scope)
        cast = Cast(this)
        watchNetwork()
    }

    /** Wi-Fi ou Ethernet : pas de limite de données */
    fun onUnmeteredNetwork(): Boolean {
        val cm = getSystemService(ConnectivityManager::class.java)
        val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
        return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
    }

    /** Sur un Wi-Fi (ou Ethernet) : seul cas où l'on tente l'adresse de ton PC sur le réseau de la maison */
    fun onWifi(): Boolean {
        val cm = getSystemService(ConnectivityManager::class.java)
        val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
        return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) || caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
    }

    /** Qualité à utiliser pour écouter en streaming maintenant */
    fun streamingQuality(): Quality =
        if (onUnmeteredNetwork()) Quality.ORIGINAL else settings.mobileQuality.value

    private val serverLock = Mutex()

    /**
     * Choisit l'adresse de ton PC. En Wi-Fi : d'abord son adresse sur le réseau de la maison (réponse rapide
     * exigée), puis HTTPS Tailscale, puis l'adresse Tailscale connue et l'adresse actuelle. En données mobiles,
     * jamais l'adresse de la maison. Partagé entre l'appli et la voiture. false si ton PC ne répond nulle part.
     */
    suspend fun findServer(): Boolean = serverLock.withLock {
        val wifi = onWifi()
        val lan = settings.lanServer
        val candidates = buildList {
            if (wifi && lan != null) add(lan to LAN_TIMEOUT)
            listOfNotNull(settings.httpsServer, settings.remoteServer, settings.server.value.ifBlank { null }).forEach { add(it to 4000) }
        }.filter { wifi || it.first != lan }.distinctBy { it.first.trimEnd('/') }

        for ((s, timeout) in candidates) {
            val info = api.pingInfoAt(s, timeout) ?: continue
            if (s == lan) return@withLock use(s, Connection.LAN)
            // Ton PC refuse peut-être le Wi-Fi : il ne l'annonce alors pas, et l'ancienne adresse est oubliée.
            val announcedLan = info.optStr("lan")?.takeIf { it.startsWith("http://") && info.optBoolean("wifi", true) }
            settings.setLanServer(announcedLan)
            val https = info.optStr("https")?.takeIf { it.startsWith("https://") }
            if (https != null) settings.setHttpsServer(https)
            // L'adresse qui marche hors de la maison, à garder même si on se relie par le Wi-Fi.
            info.optStr("tailscale")?.takeIf { it.startsWith("http") }?.let { settings.setRemoteServer(it) }
            // À la maison : on passe en direct dès que ton PC nous donne son adresse locale
            if (wifi && announcedLan != null && announcedLan != s && api.pingInfoAt(announcedLan, LAN_TIMEOUT) != null)
                return@withLock use(announcedLan, Connection.LAN)
            if (https != null && https != s.trimEnd('/') && api.pingInfoAt(https) != null)
                return@withLock use(https, Connection.TAILSCALE)
            return@withLock use(s, Connection.TAILSCALE)
        }
        _connection.value = Connection.OFFLINE
        false
    }

    private fun use(server: String, how: Connection): Boolean {
        settings.setServer(server)
        _connection.value = how
        return true
    }

    /**
     * Relie l'appli à un PC d'après l'adresse tapée. Rend null si ça marche, sinon le message à afficher.
     * Les adresses apprises d'un autre PC sont oubliées.
     */
    suspend fun connectTo(address: String): String? {
        val url = normalizeAddress(address)
        if (url.isEmpty()) return "Indique l'adresse de ton PC."
        api.pingInfoAt(url, 6000)
            ?: return "Rien ne répond à cette adresse. Vérifie que Tailscale est connecté sur le téléphone et que le serveur mobile tourne sur ton PC."
        settings.startOver(url)
        findServer()
        return null
    }

    /** Wi-Fi ↔ données mobiles : on rechoisit l'adresse de ton PC */
    private fun watchNetwork() {
        val cm = getSystemService(ConnectivityManager::class.java)
        var lastWifi: Boolean? = null
        var job: Job? = null
        fun changed() {
            job?.cancel()
            job = scope.launch {
                delay(1000)   // laisse le réseau se stabiliser
                findServer()
            }
        }
        runCatching {
            cm.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
                override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
                    val wifi = caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                        caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
                    if (wifi != lastWifi) {
                        val first = lastWifi == null
                        lastWifi = wifi
                        if (!first) scope.launch { changed() }
                    }
                }
                override fun onLost(network: Network) {
                    scope.launch {
                        changed()
                        // Plus de Wi-Fi : la diffusion s'arrête (la Mu-so ne peut plus être pilotée)
                        if (cast.target.value != null && !onWifi()) cast.lost(cast.target.value!!.name)
                    }
                }
            })
        }
    }

    companion object {
        lateinit var instance: App
            private set
        private const val LAN_TIMEOUT = 1500
        private const val DEFAULT_PORT = 8775

        /** « mon-pc.xxxx.ts.net », « 100.1.2.3 » ou une adresse déjà complète → une adresse complète, avec le port */
        fun normalizeAddress(typed: String): String {
            val t = typed.trim()
            val https = t.startsWith("https://", ignoreCase = true)
            val host = t.replace(Regex("^https?://", RegexOption.IGNORE_CASE), "").substringBefore('/').trim()
            if (host.isEmpty()) return ""
            if (https) return "https://$host"
            val hasPort = if (host.startsWith("[")) host.contains("]:") else host.contains(':')
            return "http://$host" + if (hasPort) "" else ":$DEFAULT_PORT"
        }
    }
}

private fun org.json.JSONObject.optStr(k: String): String? =
    if (!has(k) || isNull(k)) null else optString(k).trimEnd('/').ifBlank { null }

class Settings(context: Context) {
    private val prefs = context.getSharedPreferences("reglages", Context.MODE_PRIVATE)

    /** L'adresse de ton PC, vide tant que l'appli n'est pas reliée (rien n'est écrit en dur : chacun a le sien) */
    private val _server = MutableStateFlow(prefs.getString("server", "") ?: "")
    val server: StateFlow<String> = _server
    private val _mobileQuality = MutableStateFlow(
        Quality.entries.firstOrNull { it.name == prefs.getString("mobile_quality", null) } ?: Quality.HIGH)
    val mobileQuality: StateFlow<Quality> = _mobileQuality
    private val _wifiOnly = MutableStateFlow(prefs.getBoolean("wifi_only", true))
    val wifiOnly: StateFlow<Boolean> = _wifiOnly
    private val _saveToMusic = MutableStateFlow(prefs.getBoolean("save_to_music", true))
    /** Copie de chaque téléchargement dans Musique/OTOKO (visible des autres applis) */
    val saveToMusic: StateFlow<Boolean> = _saveToMusic

    /** Adresse HTTPS annoncée par ton PC (Tailscale Serve), essayée en premier */
    val httpsServer: String? get() = prefs.getString("https", null)
    /** Adresse de ton PC sur le Wi-Fi de la maison (http://192.168.x.x:8775), annoncée par ton PC */
    val lanServer: String? get() = prefs.getString("lan", null)
    /** Adresse de ton PC par Tailscale : celle qu'on a tapée, puis celle qu'il annonce */
    val remoteServer: String? get() = prefs.getString("remote", null)

    fun setServer(v: String) { _server.value = v; prefs.edit().putString("server", v).apply() }
    fun setHttpsServer(v: String) { prefs.edit().putString("https", v).apply() }
    fun setLanServer(v: String?) { prefs.edit().putString("lan", v).apply() }
    fun setRemoteServer(v: String) { prefs.edit().putString("remote", v).apply() }
    /** Un PC (peut-être un autre) : on ne garde rien de ce qu'on savait du précédent */
    fun startOver(address: String) {
        prefs.edit().remove("https").remove("lan").putString("remote", address).apply()
        setServer(address)
    }
    fun setMobileQuality(q: Quality) { _mobileQuality.value = q; prefs.edit().putString("mobile_quality", q.name).apply() }
    fun setWifiOnly(v: Boolean) { _wifiOnly.value = v; prefs.edit().putBoolean("wifi_only", v).apply() }
    fun setSaveToMusic(v: Boolean) { _saveToMusic.value = v; prefs.edit().putBoolean("save_to_music", v).apply() }
}
