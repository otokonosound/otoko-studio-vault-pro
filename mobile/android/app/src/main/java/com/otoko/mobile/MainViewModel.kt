package com.otoko.mobile

import android.app.Application
import android.content.ComponentName
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.guava.await
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONObject

enum class Screen { RELEASES, LIBRARY, OFFLINE, SHARES }

/** Ouverture de la feuille « Partager », éventuellement avec une sortie ou un morceau déjà choisi */
data class ShareRequest(
    val kind: String,
    val releases: List<Pair<Long, String>> = emptyList(),
    val tracks: List<Pair<Long, String>> = emptyList(),
    val label: String = "",
)

/** Fiche d'une sortie : le résumé s'affiche tout de suite, les morceaux arrivent ensuite */
data class ReleaseView(
    val id: Long,
    val release: Release? = null,
    val tracks: List<Track> = emptyList(),
    val loading: Boolean = true,
    val error: String? = null,
)

enum class Sort(val param: String, val label: String) {
    ADDED("added", "Ajoutés récemment"),
    TITLE("title", "Titre"),
    BPM("bpm", "BPM"),
    KEY("key", "Tonalité (Camelot)"),
    LUFS("lufs", "Volume (LUFS)"),
    DURATION("duration", "Durée"),
    PLAYS("plays", "Plus écoutés"),
}

enum class Filter(val label: String) {
    ALL("Tout"),
    REMIX("Remix et edits"),
    RELEASED("Publiés"),
    COMPATIBLE("Compatibles"),
    EVERYTHING("Tout afficher"),
}

data class UiState(
    val screen: Screen = Screen.RELEASES,
    val releases: List<Release> = emptyList(),
    val latestPublished: List<Track> = emptyList(),
    val releasesLoading: Boolean = true,
    val releasesError: String? = null,
    val releaseView: ReleaseView? = null,
    val query: String = "",
    val sort: Sort = Sort.ADDED,
    val filter: Filter = Filter.ALL,
    val tracks: List<Track> = emptyList(),
    val loading: Boolean = true,
    val error: String? = null,
    /** change à chaque nouvelle liste pour rejouer l'animation d'entrée */
    val listVersion: Int = 0,
    val current: Track? = null,
    val playing: Boolean = false,
    val position: Long = 0,
    val duration: Long = 0,
    val waveform: List<Float> = emptyList(),
    val sheetOpen: Boolean = false,
    val playingQuality: Quality? = null,
    val offlinePlayback: Boolean = false,
    val shares: List<Share> = emptyList(),
    val sharesLoading: Boolean = false,
    val sharesError: String? = null,
    /** partage public (Tailscale Funnel) allumé sur ton PC */
    val funnel: Boolean = true,
    val shareRequest: ShareRequest? = null,
)

class MainViewModel(app: Application) : AndroidViewModel(app) {
    private val a = app as App
    val settings = a.settings
    val downloads = a.downloads

    private val _ui = MutableStateFlow(UiState())
    val ui: StateFlow<UiState> = _ui.asStateFlow()

    private var controller: MediaController? = null
    private val queueTracks = mutableMapOf<String, Track>()
    private val queueQuality = mutableMapOf<String, Quality?>()
    private var loadJob: Job? = null
    private var waveJob: Job? = null

    init {
        viewModelScope.launch { findServer(); loadReleases(); reload() }
        viewModelScope.launch {
            val token = SessionToken(app, ComponentName(app, PlaybackService::class.java))
            val c = MediaController.Builder(app, token).buildAsync().await()
            controller = c
            c.addListener(object : Player.Listener {
                override fun onEvents(player: Player, events: Player.Events) = syncFromPlayer()
            })
            syncFromPlayer()
            while (isActive) {
                if (c.isPlaying) _ui.update { it.copy(position = c.currentPosition, duration = durationOf(c)) }
                delay(200)
            }
        }
    }

    // ------------------------------------------------------------ mise à jour

    /** L'adresse de la nouvelle version d'Otoko Vault, si ton PC en propose une plus récente */
    suspend fun updateUrl(): String? {
        a.findServer()
        val offered = a.api.apkCode() ?: return null
        @Suppress("DEPRECATION")
        val mine = a.packageManager.getPackageInfo(a.packageName, 0).versionCode.toLong()
        return if (offered > mine) a.api.url("/otoko.apk") else null
    }

    // ------------------------------------------------------------ bibliothèque

    private suspend fun findServer() { a.findServer() }

    /** Relie l'appli au PC de cette adresse et charge sa musique. Rend le message d'erreur, ou null. */
    suspend fun connectTo(address: String): String? {
        val problem = a.connectTo(address)
        if (problem == null) { _suggested.value = null; loadReleases(); reload() }
        return problem
    }

    private val _suggested = MutableStateFlow<String?>(null)
    /** Une adresse arrivée par « Ouvrir l'app » : à confirmer */
    val suggested: StateFlow<String?> = _suggested
    fun suggestAddress(address: String) { _suggested.value = address }
    fun dropSuggestion() { _suggested.value = null }

    fun setQuery(q: String) { _ui.update { it.copy(query = q) }; reload(debounce = true) }
    fun setSort(s: Sort) { _ui.update { it.copy(sort = s) }; reload() }
    fun setFilter(f: Filter) {
        _ui.update { it.copy(filter = if (it.filter == f) Filter.ALL else f) }
        reload()
    }
    fun setScreen(s: Screen) {
        _ui.update { it.copy(screen = s, releaseView = null) }
        if (s == Screen.RELEASES && (_ui.value.releasesError != null || _ui.value.releases.isEmpty())) loadReleases()
        if (s == Screen.SHARES) loadShares()
    }

    // ------------------------------------------------------------ partages

    fun loadShares() {
        viewModelScope.launch {
            _ui.update { it.copy(sharesLoading = true) }
            try {
                val info = a.api.shares()
                if (_ui.value.releases.isEmpty()) loadReleases()
                _ui.update { it.copy(shares = info.shares, funnel = info.funnel, sharesLoading = false, sharesError = null) }
            } catch (e: Exception) {
                _ui.update { it.copy(sharesLoading = false, sharesError = e.message ?: "Les partages demandent Tailscale connecté.") }
            }
        }
    }

    fun openShare(req: ShareRequest) {
        if (_ui.value.releases.isEmpty()) loadReleases()
        _ui.update { it.copy(shareRequest = req) }
    }

    fun closeShare() {
        _ui.update { it.copy(shareRequest = null) }
        if (_ui.value.screen == Screen.SHARES) loadShares()
    }

    /** Morceaux pour la recherche de la feuille « Partager » (toute la bibliothèque) */
    suspend fun searchTracks(q: String): List<Track> =
        runCatching { a.api.tracks(mapOf("scope" to "all", "limit" to "8", "q" to q)) }.getOrDefault(emptyList())

    /** Crée le lien ; renvoie le partage et l'état du partage public, ou l'erreur */
    fun createShare(body: JSONObject, onDone: (Share, Boolean) -> Unit, onError: (String) -> Unit) {
        viewModelScope.launch {
            try {
                val s = a.api.createShare(body)
                val funnel = runCatching { a.api.sharesFunnel() }.getOrDefault(true)
                _ui.update { it.copy(funnel = funnel) }
                onDone(s, funnel)
            } catch (e: Exception) {
                onError(e.message ?: "Les partages demandent Tailscale connecté.")
            }
        }
    }

    /** Désactiver, réactiver, prolonger, supprimer */
    fun shareAction(s: Share, action: String, onMessage: (String) -> Unit) {
        viewModelScope.launch {
            try {
                when (action) {
                    "off" -> a.api.updateShare(s.id, JSONObject().put("revoked", true))
                    "on" -> a.api.updateShare(s.id, JSONObject().put("revoked", false))
                    "extend" -> a.api.updateShare(s.id, JSONObject().put("expires_days", 30).put("revoked", false))
                    "delete" -> a.api.deleteShare(s.id)
                }
                onMessage(when (action) {
                    "off" -> "Lien désactivé."
                    "on" -> "Lien réactivé."
                    "extend" -> "Lien prolongé de 30 jours."
                    else -> "Lien supprimé."
                })
                loadShares()
            } catch (e: Exception) {
                onMessage(e.message ?: "Les partages demandent Tailscale connecté.")
            }
        }
    }

    // ------------------------------------------------------------ sorties

    fun loadReleases() {
        viewModelScope.launch {
            _ui.update { it.copy(releasesLoading = true) }
            try {
                val list = a.api.releases()
                val latest = runCatching {
                    a.api.tracks(mapOf("status" to "released", "sort" to "published", "limit" to "15"))
                }.getOrDefault(emptyList())
                _ui.update { it.copy(releases = list, latestPublished = latest, releasesLoading = false, releasesError = null) }
            } catch (e: Exception) {
                _ui.update { it.copy(releasesLoading = false, releasesError = "Ton PC ne répond pas.") }
            }
        }
    }

    fun openRelease(id: Long) {
        if (id <= 0) return
        val known = _ui.value.releases.firstOrNull { it.id == id }
        _ui.update { it.copy(releaseView = ReleaseView(id, known), sheetOpen = false) }
        viewModelScope.launch {
            val v = try {
                val (r, tracks) = a.api.release(id)
                ReleaseView(id, r, tracks, loading = false)
            } catch (e: Exception) {
                ReleaseView(id, known, loading = false, error = "Ton PC ne répond pas.")
            }
            if (_ui.value.releaseView?.id == id) _ui.update { it.copy(releaseView = v) }
        }
    }

    fun closeRelease() = _ui.update { it.copy(releaseView = null) }

    /** Joue toute la sortie dans l'ordre (la file d'attente devient la sortie) */
    fun playRelease(id: Long, onError: (String) -> Unit) {
        val v = _ui.value.releaseView
        if (v?.id == id && v.tracks.isNotEmpty()) { play(v.tracks, 0); return }
        viewModelScope.launch {
            try {
                val tracks = a.api.release(id).second
                if (tracks.isEmpty()) onError("Cette sortie n'a pas encore de morceaux.") else play(tracks, 0)
            } catch (e: Exception) {
                onError("Ton PC ne répond pas.")
            }
        }
    }
    fun openSheet(open: Boolean) = _ui.update { it.copy(sheetOpen = open && it.current != null) }

    fun reload(debounce: Boolean = false) {
        loadJob?.cancel()
        loadJob = viewModelScope.launch {
            if (debounce) delay(250)
            val s = _ui.value
            _ui.update { it.copy(loading = true) }
            val params = buildMap {
                put("q", s.query)
                put("sort", s.sort.param)
                when (s.filter) {
                    Filter.ALL -> {}
                    Filter.REMIX -> put("nature", "remix,edit,mashup")
                    Filter.RELEASED -> put("status", "released")
                    Filter.COMPATIBLE -> s.current?.let { put("compatible_with", it.id.toString()) }
                    Filter.EVERYTHING -> put("scope", "all")
                }
            }
            try {
                val list = a.api.tracks(params)
                _ui.update { it.copy(tracks = list, loading = false, error = null, listVersion = it.listVersion + 1) }
            } catch (e: Exception) {
                if (!isActive) return@launch
                // Sans ton PC : on montre ce qui est sur le téléphone
                val q = s.query.lowercase()
                val local = downloads.saved.value.filter { q.isBlank() || it.title.lowercase().contains(q) }
                _ui.update {
                    it.copy(tracks = local, loading = false, listVersion = it.listVersion + 1,
                        error = "Ton PC ne répond pas : seuls les morceaux hors ligne sont affichés.")
                }
            }
        }
    }

    // ------------------------------------------------------------ lecture

    fun play(list: List<Track>, index: Int) {
        val c = controller ?: return
        val streamQuality = a.streamingQuality()
        queueTracks.clear()
        queueQuality.clear()
        val items = list.map { t ->
            val local = downloads.localFile(t.id)
            queueTracks[t.id.toString()] = t
            queueQuality[t.id.toString()] = if (local != null) null else streamQuality
            // Même construction que pour Android Auto (pochette content://, infos du morceau en extras)
            buildTrackItem(a, t, t.id.toString(), withUri = true)
        }
        c.setMediaItems(items, index, 0)
        c.prepare()
        c.play()
    }

    fun togglePlay() {
        val c = controller ?: return
        if (c.isPlaying) c.pause() else c.play()
    }

    fun next() = controller?.seekToNextMediaItem()
    fun previous() {
        val c = controller ?: return
        if (c.currentPosition > 3000 || !c.hasPreviousMediaItem()) c.seekTo(0) else c.seekToPreviousMediaItem()
    }

    fun seekFraction(f: Float) {
        val c = controller ?: return
        val d = durationOf(c)
        if (d > 0) {
            c.seekTo((d * f.coerceIn(0f, 1f)).toLong())
            _ui.update { it.copy(position = c.currentPosition) }
        }
    }

    private fun durationOf(c: MediaController): Long {
        val d = c.duration
        return if (d > 0) d else ((_ui.value.current?.duration ?: 0.0) * 1000).toLong()
    }

    private fun syncFromPlayer() {
        val c = controller ?: return
        val item = c.currentMediaItem
        val id = item?.mediaId
        // Lancé depuis la voiture ou repris : les infos du morceau voyagent avec lui
        val track = id?.let { queueTracks[it] } ?: item?.mediaMetadata?.extras?.getString(TRACK_JSON)
            ?.let { runCatching { Track.fromJson(JSONObject(it)) }.getOrNull() }
        val changed = track?.id != _ui.value.current?.id
        _ui.update {
            it.copy(
                current = track ?: it.current,
                playing = c.isPlaying,
                position = c.currentPosition,
                duration = durationOf(c),
                playingQuality = id?.let { k -> queueQuality[k] },
                offlinePlayback = id != null && queueQuality.containsKey(id) && queueQuality[id] == null,
                waveform = if (changed) emptyList() else it.waveform,
            )
        }
        if (changed && track != null) {
            loadWaveform(track)
            if (_ui.value.filter == Filter.COMPATIBLE) reload()
        }
    }

    private fun loadWaveform(t: Track) {
        waveJob?.cancel()
        waveJob = viewModelScope.launch {
            val w = runCatching { a.api.waveform(t.id, 64) }.getOrNull() ?: return@launch
            if (_ui.value.current?.id == t.id) _ui.update { it.copy(waveform = w.peaks) }
        }
    }

    // ------------------------------------------------------------ hors ligne

    /** null si le téléchargement peut démarrer, sinon la raison */
    fun download(t: Track): String? {
        wifiOnlyBlock()?.let { return it }
        downloads.start(t)
        return null
    }

    private fun wifiOnlyBlock(): String? =
        if (settings.wifiOnly.value && !a.onUnmeteredNetwork())
            "Téléchargement en Wi-Fi seulement : désactive l'option dans Hors ligne pour utiliser tes données."
        else null

    val connection = a.connection

    // ------------------------------------------------------------ diffusion (Mu-so)

    val cast = a.cast
    fun onWifi() = a.onWifi()
    fun castTo(d: UpnpDevice): String? = cast.castTo(d)
    fun stopCasting() = cast.stopCasting()

    /** Toute la sortie hors ligne, un titre à la fois (les morceaux viennent du serveur si besoin) */
    fun downloadRelease(id: Long, known: List<Track>? = null, onMessage: (String) -> Unit) {
        wifiOnlyBlock()?.let { onMessage(it); return }
        viewModelScope.launch {
            val tracks = known?.takeIf { it.isNotEmpty() } ?: try {
                a.api.release(id).second
            } catch (e: Exception) {
                onMessage("Ton PC ne répond pas."); return@launch
            }
            when {
                tracks.isEmpty() -> onMessage("Cette sortie n'a pas encore de morceaux.")
                tracks.all { downloads.states.value[it.id] == DlState.Done } -> onMessage("Cette sortie est déjà sur le téléphone.")
                else -> {
                    downloads.startRelease(id, tracks)
                    if (known == null) onMessage("Téléchargement de la sortie lancé (${tracks.size} titres).")
                }
            }
        }
    }

    fun saveToMusic(t: Track, onMessage: (String) -> Unit) {
        viewModelScope.launch {
            onMessage(downloads.saveToMusic(t) ?: "Enregistré dans Musique › OTOKO.")
        }
    }

    fun saveAllToMusic(onMessage: (String) -> Unit) {
        viewModelScope.launch {
            val (ok, ko) = downloads.saveAllToMusic()
            onMessage(when {
                ok == 0 && ko == 0 -> "Tout est déjà dans Musique."
                ko == 0 -> "$ok morceau${if (ok > 1) "x" else ""} enregistré${if (ok > 1) "s" else ""} dans Musique › OTOKO."
                else -> "$ok enregistré${if (ok > 1) "s" else ""}, $ko en échec."
            })
        }
    }

    override fun onCleared() {
        controller?.release()
        super.onCleared()
    }
}
