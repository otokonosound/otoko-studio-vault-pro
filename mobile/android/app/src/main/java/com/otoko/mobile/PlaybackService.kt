package com.otoko.mobile

import android.app.PendingIntent
import android.content.Intent
import android.os.Bundle
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaLibraryService.LibraryParams
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSession.MediaItemsWithStartPosition
import androidx.media3.session.SessionError
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.guava.future
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Lecture en arrière-plan : continue écran éteint, contrôles sur l'écran verrouillé,
 * dans les notifications, au casque Bluetooth et dans la voiture.
 * C'est aussi la bibliothèque parcourue par Android Auto (voir MediaTree).
 */
class PlaybackService : MediaLibraryService() {
    private var session: MediaLibrarySession? = null
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private lateinit var tree: MediaTree

    override fun onCreate() {
        super.onCreate()
        val app = application as App
        tree = MediaTree(app)
        val player = ExoPlayer.Builder(this)
            .setAudioAttributes(
                AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(),
                /* handleAudioFocus = */ true)
            .setHandleAudioBecomingNoisy(true)   // pause quand on débranche le casque
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .build()
        player.addListener(object : Player.Listener {
            override fun onEvents(player: Player, events: Player.Events) {
                if (events.containsAny(Player.EVENT_MEDIA_ITEM_TRANSITION, Player.EVENT_TIMELINE_CHANGED,
                        Player.EVENT_IS_PLAYING_CHANGED)) saveQueue(player)
            }
        })
        val open = PendingIntent.getActivity(this, 0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        session = MediaLibrarySession.Builder(this, player, LibraryCallback())
            .setSessionActivity(open)
            .build()
        local = player
        mainScope.launch { app.cast.target.collect { switchTo(it) } }
    }

    private lateinit var local: ExoPlayer
    private var remote: UpnpPlayer? = null
    private val mainScope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    /**
     * Téléphone ↔ lecteur réseau : même file, même morceau, même position.
     * La session garde son identité : notification, casque et voiture suivent le nouveau lecteur.
     */
    private fun switchTo(device: UpnpDevice?) {
        val s = session ?: return
        val app = application as App
        if (device != null && remote?.device?.udn == device.udn) return
        if (device == null && remote == null) return
        val from = s.player
        val items = (0 until from.mediaItemCount).map { from.getMediaItemAt(it) }
        val index = from.currentMediaItemIndex
        val position = from.currentPosition
        val wasPlaying = from.playWhenReady
        val old = remote
        if (device == null) {
            remote = null
            old?.stopRemote()
            // Retour sur le téléphone au même endroit ; en pause si la Mu-so a été perdue
            val lost = old != null && !app.onWifi() || lostFlag
            lostFlag = false
            if (items.isNotEmpty()) {
                local.setMediaItems(items, index, position)
                local.prepare()
                local.playWhenReady = wasPlaying && !lost
            }
            s.player = local
            old?.release()
            return
        }
        val lan = app.settings.lanServer ?: return
        val r = UpnpPlayer(app, device, lan,
            onLost = { lostFlag = true; app.cast.lost(device.name) },
            onMessage = { app.cast.message(it) })
        if (items.isNotEmpty()) {
            r.setMediaItems(items, index, position)
            r.prepare()
            r.playWhenReady = wasPlaying
        }
        local.pause()
        remote = r
        s.player = r
        old?.let { it.stopRemote(); it.release() }
    }

    private var lostFlag = false

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? = session

    /** La file en cours, pour reprendre la lecture plus tard (voiture, casque, notification) */
    private fun saveQueue(player: Player) {
        if (player.mediaItemCount == 0) return
        val tracks = mutableListOf<Track>()
        var index = 0
        for (i in 0 until player.mediaItemCount) {
            val json = player.getMediaItemAt(i).mediaMetadata.extras?.getString(TRACK_JSON) ?: continue
            val t = runCatching { Track.fromJson(JSONObject(json)) }.getOrNull() ?: continue
            if (i == player.currentMediaItemIndex) index = tracks.size
            tracks += t
        }
        val position = player.currentPosition
        val app = application as App
        scope.launch { LastQueue.save(app, tracks, index, position) }
    }

    private inner class LibraryCallback : MediaLibrarySession.Callback {
        private val app get() = application as App

        override fun onGetLibraryRoot(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, params: LibraryParams?,
        ): ListenableFuture<LibraryResult<MediaItem>> {
            val extras = Bundle().apply {
                putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM)
                putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM)
                putBoolean("android.media.browse.SEARCH_SUPPORTED", true)
            }
            val outParams = LibraryParams.Builder().setExtras(extras).build()
            // Reprise de lecture : une racine qui ne contient que le dernier morceau joué
            if (params?.isRecent == true) {
                if (LastQueue.load(app) == null)
                    return immediate(LibraryResult.ofError(SessionError(SessionError.ERROR_NOT_SUPPORTED, "Rien à reprendre")))
                return immediate(LibraryResult.ofItem(tree.recentRoot(), LibraryParams.Builder().setRecent(true).build()))
            }
            return immediate(LibraryResult.ofItem(tree.root(), outParams))
        }

        override fun onGetChildren(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String,
            page: Int, pageSize: Int, params: LibraryParams?,
        ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = scope.future {
            try {
                LibraryResult.ofItemList(tree.children(parentId, page, pageSize), params)
            } catch (e: Exception) {
                // Ton PC ne répond pas : on le dit au lieu d'une liste vide (« Hors ligne » marche toujours)
                LibraryResult.ofItemList(if (page <= 0) listOf(tree.unreachable()) else emptyList(), params)
            }
        }

        override fun onGetItem(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String,
        ): ListenableFuture<LibraryResult<MediaItem>> = scope.future {
            val item = runCatching { tree.item(mediaId) }.getOrNull()
            if (item != null) LibraryResult.ofItem(item, null)
            else LibraryResult.ofError(SessionError(SessionError.ERROR_BAD_VALUE, "Élément introuvable"))
        }

        override fun onSearch(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, params: LibraryParams?,
        ): ListenableFuture<LibraryResult<Void>> = scope.future {
            val count = tree.search(query).size
            session.notifySearchResultChanged(browser, query, count, params)
            LibraryResult.ofVoid()
        }

        override fun onGetSearchResult(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String,
            page: Int, pageSize: Int, params: LibraryParams?,
        ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = scope.future {
            if (tree.searchItems().isEmpty()) tree.search(query)
            val all = tree.searchItems()
            val from = if (pageSize in 1 until Int.MAX_VALUE) page.coerceAtLeast(0) * pageSize else 0
            val to = if (pageSize in 1 until Int.MAX_VALUE) minOf(all.size, from + pageSize) else all.size
            LibraryResult.ofItemList(if (from < all.size) all.subList(from, to) else emptyList(), params)
        }

        override fun onAddMediaItems(
            mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>,
        ): ListenableFuture<MutableList<MediaItem>> = scope.future {
            mediaItems.map { tree.resolve(it) }.filter { it.localConfiguration != null }.toMutableList()
        }

        override fun onSetMediaItems(
            mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>,
            startIndex: Int, startPositionMs: Long,
        ): ListenableFuture<MediaItemsWithStartPosition> = scope.future {
            // L'appli envoie des morceaux déjà prêts ; la voiture n'envoie que des identifiants
            if (mediaItems.all { it.localConfiguration != null }) {
                MediaItemsWithStartPosition(mediaItems, startIndex, startPositionMs)
            } else {
                val (items, index) = tree.resolveQueue(mediaItems, startIndex)
                // Une demande sans morceau (une commande vocale, par exemple) est ignorée
                if (items.isEmpty()) throw UnsupportedOperationException("Rien à jouer")
                val resume = if (mediaItems.size == 1 && MediaTree.parseTrackId(mediaItems[0].mediaId)?.first == MediaTree.RECENT)
                    LastQueue.load(app)?.positionMs ?: C.TIME_UNSET else startPositionMs
                MediaItemsWithStartPosition(items, index, resume)
            }
        }

        override fun onPlaybackResumption(
            mediaSession: MediaSession, controller: MediaSession.ControllerInfo,
        ): ListenableFuture<MediaItemsWithStartPosition> = scope.future {
            val q = LastQueue.load(app) ?: throw UnsupportedOperationException("Rien à reprendre")
            MediaItemsWithStartPosition(
                q.tracks.map { tree.playable(it, "${MediaTree.RECENT}/track:${it.id}") }, q.index, q.positionMs)
        }
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        mainScope.cancel()
        remote?.let { it.stopRemote(); it.release() }
        remote = null
        session?.run {
            if (::local.isInitialized) local.release()
            release()
        }
        session = null
        scope.cancel()
        super.onDestroy()
    }
}

private fun <T> immediate(v: T): ListenableFuture<T> = com.google.common.util.concurrent.Futures.immediateFuture(v)
