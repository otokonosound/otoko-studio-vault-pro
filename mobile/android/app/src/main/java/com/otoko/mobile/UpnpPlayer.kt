package com.otoko.mobile

import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.DeviceInfo
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.URL

/**
 * Lecture sur un lecteur réseau UPnP (Naim Mu-so) vue comme un Player Media3 : la notification,
 * l'écran verrouillé, le casque et Android Auto pilotent la Mu-so comme ils pilotent le téléphone.
 * La Mu-so lit directement sur ton PC par le Wi-Fi de la maison (adresse « lan »), en qualité Origine.
 */
class UpnpPlayer(
    private val app: App,
    val device: UpnpDevice,
    private val lanBase: String,
    /** La Mu-so ne répond plus, ou un message à afficher */
    private val onLost: () -> Unit,
    private val onMessage: (String) -> Unit,
) : SimpleBasePlayer(Looper.getMainLooper()) {

    private val io = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val net = Mutex()   // une commande SOAP à la fois
    private var items: List<MediaItem> = emptyList()
    private var index = 0
    private var playWhenReady = false
    private var state = Player.STATE_IDLE
    private var positionMs = 0L
    private var durationMs = C.TIME_UNSET
    private var volume = 30
    private var loadedIndex = -1        // morceau effectivement envoyé à la Mu-so
    private var remotePlaying = false   // dernier état connu : en lecture
    private var loadJob: Job? = null
    private var failures = 0
    private var released = false

    init {
        io.launch { poll() }
    }

    override fun getState(): State {
        val playlist = items.mapIndexed { i, item ->
            MediaItemData.Builder("$i:${item.mediaId}")
                .setMediaItem(item)
                .setDurationUs(if (i == index && durationMs > 0) durationMs * 1000
                    else item.mediaMetadata.durationMs?.let { it * 1000 } ?: C.TIME_UNSET)
                .setIsSeekable(true)
                .build()
        }
        val playing = playWhenReady && state == Player.STATE_READY
        return State.Builder()
            .setAvailableCommands(COMMANDS)
            .setPlaylist(playlist)
            .setCurrentMediaItemIndex(if (items.isEmpty()) 0 else index.coerceIn(0, items.size - 1))
            .setPlayWhenReady(playWhenReady, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(if (items.isEmpty()) Player.STATE_IDLE else state)
            .setContentPositionMs(PositionSupplier.getExtrapolating(positionMs, if (playing) 1f else 0f))
            .setDeviceInfo(DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE).setMinVolume(0).setMaxVolume(100).build())
            .setDeviceVolume(volume)
            .build()
    }

    // ------------------------------------------------------------ commandes reçues

    override fun handleSetMediaItems(mediaItems: MutableList<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<*> {
        items = mediaItems.toList()
        index = if (startIndex == C.INDEX_UNSET) 0 else startIndex.coerceIn(0, maxOf(0, items.size - 1))
        positionMs = if (startPositionMs == C.TIME_UNSET) 0 else startPositionMs
        loadedIndex = -1
        if (state != Player.STATE_IDLE && items.isNotEmpty()) load(index, positionMs)
        return done()
    }

    override fun handleAddMediaItems(i: Int, mediaItems: MutableList<MediaItem>): ListenableFuture<*> {
        items = items.toMutableList().apply { addAll(i.coerceIn(0, size), mediaItems) }
        if (i <= index && items.size > mediaItems.size) { index += mediaItems.size; loadedIndex += mediaItems.size }
        return done()
    }

    override fun handleRemoveMediaItems(from: Int, to: Int): ListenableFuture<*> {
        val removingCurrent = index in from until to
        items = items.toMutableList().apply { subList(from, to).clear() }
        if (index >= to) { index -= (to - from); loadedIndex -= (to - from) }
        else if (removingCurrent) {
            index = from.coerceAtMost(maxOf(0, items.size - 1))
            loadedIndex = -1
            if (items.isNotEmpty() && state != Player.STATE_IDLE) load(index, 0)
        }
        return done()
    }

    override fun handlePrepare(): ListenableFuture<*> {
        if (items.isNotEmpty()) {
            state = Player.STATE_BUFFERING
            if (loadedIndex != index) load(index, positionMs)
        }
        return done()
    }

    override fun handleSetPlayWhenReady(play: Boolean): ListenableFuture<*> {
        playWhenReady = play
        if (play && state == Player.STATE_ENDED) { index = 0; positionMs = 0; load(0, 0) }
        else if (loadedIndex == index) command { if (play) avt("Play", "Speed" to "1") else avt("Pause") }
        return done()
    }

    override fun handleStop(): ListenableFuture<*> {
        playWhenReady = false
        state = Player.STATE_IDLE
        loadedIndex = -1
        command { avt("Stop") }
        return done()
    }

    override fun handleSeek(mediaItemIndex: Int, pos: Long, seekCommand: Int): ListenableFuture<*> {
        val target = if (mediaItemIndex == C.INDEX_UNSET) index else mediaItemIndex
        val p = if (pos == C.TIME_UNSET) 0 else pos
        if (target != index || loadedIndex != index) {
            index = target.coerceIn(0, maxOf(0, items.size - 1))
            positionMs = p
            durationMs = C.TIME_UNSET
            load(index, p)
        } else {
            positionMs = p
            command { avt("Seek", "Unit" to "REL_TIME", "Target" to Upnp.hms(p)) }
        }
        return done()
    }

    override fun handleSetDeviceVolume(v: Int, flags: Int): ListenableFuture<*> {
        volume = v.coerceIn(0, 100)
        val target = volume
        command { rc("SetVolume", "Channel" to "Master", "DesiredVolume" to "$target") }
        return done()
    }

    override fun handleIncreaseDeviceVolume(flags: Int) = handleSetDeviceVolume(volume + 3, flags)
    override fun handleDecreaseDeviceVolume(flags: Int) = handleSetDeviceVolume(volume - 3, flags)

    override fun handleRelease(): ListenableFuture<*> {
        released = true
        io.cancel()
        return done()
    }

    /** En quittant la Mu-so : on l'arrête (sans attendre) puis on libère */
    fun stopRemote() {
        val dev = device
        CoroutineScope(Dispatchers.IO).launch {
            runCatching { Upnp.soap(dev.avTransport, Upnp.AV_TRANSPORT, "Stop", listOf("InstanceID" to "0")) }
        }
    }

    /** Position et index actuels (pour reprendre sur le téléphone au même endroit) */
    val snapshot: Triple<List<MediaItem>, Int, Long> get() = Triple(items, index, currentPosition)

    // ------------------------------------------------------------ envoi d'un morceau

    private fun load(i: Int, startMs: Long) {
        val item = items.getOrNull(i) ?: return
        state = Player.STATE_BUFFERING
        loadJob?.cancel()
        loadJob = io.launch {
            val trackId = MediaTree.parseTrackId(item.mediaId)?.second
            // « dlna=1 » : ton PC annonce le FLAC en « audio/x-flac », le type que déclare la Mu-so
            val url = trackId?.let { "${lanBase.trimEnd('/')}/api/tracks/$it/audio?quality=lossless&dlna=1" }
            // HEAD : ton PC prépare la version FLAC si besoin et donne le type exact (flac, mp3…)
            val mime = url?.let { head(it) }
            if (url == null || mime == null) {
                main {
                    onMessage("« ${item.mediaMetadata.title ?: "Ce morceau"} » n'est pas joignable sur ton PC : morceau suivant.")
                    if (i == index) next(auto = true)
                }
                return@launch
            }
            val ok = runCatching {
                net.withLock {
                    avtRaw("SetAVTransportURI", "CurrentURI" to url, "CurrentURIMetaData" to didl(item, trackId, url, mime))
                    if (startMs > 2000) runCatching { avtRaw("Seek", "Unit" to "REL_TIME", "Target" to Upnp.hms(startMs)) }
                    if (playWhenReady) avtRaw("Play", "Speed" to "1")
                }
            }.isSuccess
            main {
                if (i != index) return@main
                if (ok) {
                    loadedIndex = i
                    positionMs = startMs
                    durationMs = item.mediaMetadata.durationMs ?: C.TIME_UNSET
                    state = Player.STATE_READY
                    remotePlaying = false
                } else {
                    onMessage("La Mu-so refuse ce morceau.")
                    state = Player.STATE_READY
                    playWhenReady = false
                }
            }
        }
    }

    private fun next(auto: Boolean) {
        if (index + 1 < items.size) {
            index++
            positionMs = 0
            durationMs = C.TIME_UNSET
            load(index, 0)
        } else {
            state = Player.STATE_ENDED
            if (auto) playWhenReady = false
        }
        invalidateState()
    }

    // ------------------------------------------------------------ suivi de la Mu-so

    private suspend fun poll() {
        var tick = 0
        while (io.isActive) {
            delay(1000)
            if (released) return
            try {
                val (transport, pos, dur) = net.withLock {
                    val t = Upnp.value(avtRaw("GetTransportInfo"), "CurrentTransportState") ?: ""
                    val info = avtRaw("GetPositionInfo")
                    Triple(t, Upnp.parseHms(Upnp.value(info, "RelTime")), Upnp.parseHms(Upnp.value(info, "TrackDuration")))
                }
                val vol = if (tick++ % 5 == 0 && device.renderingControl != null) runCatching {
                    net.withLock { Upnp.value(rcRaw("GetVolume", "Channel" to "Master"), "CurrentVolume")?.toIntOrNull() }
                }.getOrNull() else null
                failures = 0
                main { onRemote(transport, pos, dur, vol) }
            } catch (e: Exception) {
                if (++failures >= 6) {   // ~6 s sans réponse
                    main { onLost() }
                    return
                }
            }
        }
    }

    private fun onRemote(transport: String, pos: Long?, dur: Long?, vol: Int?) {
        vol?.let { volume = it }
        if (loadedIndex == index && loadJob?.isActive != true) {
            pos?.let { positionMs = it }
            dur?.takeIf { it > 0 }?.let { durationMs = it }
            when (transport) {
                "PLAYING" -> { remotePlaying = true; state = Player.STATE_READY; playWhenReady = true }
                "PAUSED_PLAYBACK" -> { remotePlaying = false; state = Player.STATE_READY; playWhenReady = false }
                "TRANSITIONING" -> state = Player.STATE_BUFFERING
                "STOPPED", "NO_MEDIA_PRESENT" -> {
                    // Fin du morceau (on jouait, et rien n'a demandé l'arrêt) : morceau suivant
                    if (remotePlaying && playWhenReady) {
                        remotePlaying = false
                        next(auto = true)
                        return
                    }
                    remotePlaying = false
                }
            }
        }
        invalidateState()
    }

    // ------------------------------------------------------------ outils

    private fun command(block: () -> Unit) {
        io.launch { runCatching { net.withLock { block() } } }
    }

    private fun avt(action: String, vararg args: Pair<String, String>) { avtRaw(action, *args) }
    private fun rc(action: String, vararg args: Pair<String, String>) { rcRaw(action, *args) }

    private fun avtRaw(action: String, vararg args: Pair<String, String>): String =
        Upnp.soap(device.avTransport, Upnp.AV_TRANSPORT, action, listOf("InstanceID" to "0") + args)

    private fun rcRaw(action: String, vararg args: Pair<String, String>): String =
        Upnp.soap(device.renderingControl ?: error("Pas de réglage de volume"), Upnp.RENDERING, action,
            listOf("InstanceID" to "0") + args)

    private fun head(url: String): String? = runCatching {
        val c = URL(url).openConnection() as HttpURLConnection
        c.requestMethod = "HEAD"
        c.connectTimeout = 4000
        c.readTimeout = 120000   // la première conversion FLAC d'un long morceau peut prendre un moment
        try {
            if (c.responseCode != 200) null
            else c.contentType?.substringBefore(';')?.trim()?.let { if (it == "audio/flac") "audio/x-flac" else it }
        } finally {
            c.disconnect()
        }
    }.getOrNull()

    private fun didl(item: MediaItem, trackId: Long, url: String, mime: String): String {
        val m = item.mediaMetadata
        val e = Upnp::escape
        val dur = m.durationMs?.let { " duration=\"${Upnp.hms(it)}.000\"" } ?: ""
        return "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" " +
            "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">" +
            "<item id=\"sv-$trackId\" parentID=\"0\" restricted=\"1\">" +
            "<dc:title>${e(m.title?.toString() ?: "Sans titre")}</dc:title>" +
            "<dc:creator>${e(m.artist?.toString() ?: "")}</dc:creator>" +
            "<upnp:artist>${e(m.artist?.toString() ?: "")}</upnp:artist>" +
            (m.albumTitle?.let { "<upnp:album>${e(it.toString())}</upnp:album>" } ?: "") +
            "<upnp:albumArtURI>${e("${lanBase.trimEnd('/')}/api/tracks/$trackId/cover?size=600")}</upnp:albumArtURI>" +
            "<upnp:class>object.item.audioItem.musicTrack</upnp:class>" +
            "<res protocolInfo=\"http-get:*:$mime:*\"$dur>${e(url)}</res>" +
            "</item></DIDL-Lite>"
    }

    private fun done(): ListenableFuture<*> = Futures.immediateVoidFuture()

    private suspend fun main(block: () -> Unit) = withContext(Dispatchers.Main) {
        if (!released) { block(); invalidateState() }
    }

    companion object {
        private val COMMANDS = Player.Commands.Builder().addAll(
            Player.COMMAND_PLAY_PAUSE, Player.COMMAND_PREPARE, Player.COMMAND_STOP,
            Player.COMMAND_SEEK_TO_DEFAULT_POSITION, Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
            Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM, Player.COMMAND_SEEK_TO_PREVIOUS,
            Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM, Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_MEDIA_ITEM,
            Player.COMMAND_GET_CURRENT_MEDIA_ITEM, Player.COMMAND_GET_TIMELINE, Player.COMMAND_GET_METADATA,
            Player.COMMAND_SET_MEDIA_ITEM, Player.COMMAND_CHANGE_MEDIA_ITEMS, Player.COMMAND_RELEASE,
            Player.COMMAND_GET_DEVICE_VOLUME, Player.COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS,
            Player.COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS,
        ).build()
    }
}
