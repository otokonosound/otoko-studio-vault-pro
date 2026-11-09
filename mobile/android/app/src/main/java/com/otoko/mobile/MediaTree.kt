package com.otoko.mobile

import android.net.Uri
import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.session.MediaConstants
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap

/**
 * L'arborescence vue par Android Auto (et tout autre navigateur de médias) :
 *   racine ─ Sorties ─ une sortie ─ ses morceaux
 *          ├ Derniers publiés
 *          ├ Bibliothèque ─ Publiés, Terminés, En cours, Idées ─ leurs morceaux
 *          └ Hors ligne
 * Identifiants : « releases », « release:12 », « latest », « library », « library:done », « offline »,
 * et pour un morceau « <dossier>/track:345 » (le dossier sert à remplir la file d'attente).
 *
 * Android Auto ne demande pas les listes par pages et Media3 coupe sans prévenir celles qui dépassent
 * 256 Ko : d'où les dossiers par statut et des éléments de liste légers (voir buildTrackItem).
 */
class MediaTree(private val app: App) {
    private val api get() = app.api

    /** Dernières listes chargées, par dossier : servent à remplir la file quand on lance un titre */
    private val lists = ConcurrentHashMap<String, List<Track>>()
    private val releaseCache = ConcurrentHashMap<Long, Release>()

    @Volatile private var catalog: List<Track> = emptyList()
    @Volatile private var catalogAt = 0L

    // ------------------------------------------------------------ dossiers

    fun root(): MediaItem = folder(ROOT, "Otoko Vault", null, MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)

    fun rootChildren(): List<MediaItem> = listOf(
        folder(RELEASES, "Sorties", "ic_auto_sorties", MediaMetadata.MEDIA_TYPE_FOLDER_ALBUMS,
            browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM),
        folder(LATEST, "Derniers publiés", "ic_auto_nouveautes", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED),
        folder(LIBRARY, "Bibliothèque", "ic_auto_bibliotheque", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED,
            browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM),
        folder(OFFLINE, "Hors ligne", "ic_auto_hors_ligne", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED),
    )

    /** Racine « récents » pour la reprise de lecture : le dernier morceau joué */
    fun recentRoot(): MediaItem = folder(RECENT, "Récents", null, MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)

    fun recentChildren(): List<MediaItem> {
        val q = LastQueue.load(app) ?: return emptyList()
        val t = q.tracks.getOrNull(q.index) ?: return emptyList()
        return listOf(trackItem(t, "$RECENT/track:${t.id}"))
    }

    // ------------------------------------------------------------ contenus (réseau, hors thread principal)

    suspend fun releases(): List<MediaItem> {
        val list = withServer { api.releases() }
        list.forEach { releaseCache[it.id] = it }
        return list.map { releaseItem(it) }
    }

    /** Toute la bibliothèque, dans l'ordre de l'appli, gardée 5 minutes : changer de dossier ne recharge rien */
    private suspend fun catalog(): List<Track> {
        val now = System.currentTimeMillis()
        if (catalog.isNotEmpty() && now - catalogAt < CATALOG_TTL) return catalog
        return try {
            withServer { api.tracks(mapOf("sort" to Sort.ADDED.param, "limit" to "5000")) }
                .also { catalog = it; catalogAt = now }
        } catch (e: Exception) {
            // Zone sans réseau : on continue de naviguer dans ce qu'on connaît déjà
            catalog.ifEmpty { throw e }
        }
    }

    /** Un dossier par statut présent, du plus fini au moins fini */
    private suspend fun libraryFolders(): List<MediaItem> =
        catalog().groupBy { it.status }.entries
            .sortedBy { (status, _) -> STATUS_ORDER.indexOf(status).let { if (it < 0) STATUS_ORDER.size else it } }
            .map { (status, tracks) ->
                folder(STATUS_PREFIX + status, STATUS_TITLES[status] ?: tracks.first().statusLabel.ifBlank { "Autres" },
                    "ic_auto_bibliotheque", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED,
                    subtitle = if (tracks.size > 1) "${tracks.size} morceaux" else "1 morceau")
            }

    suspend fun tracksOf(parent: String): List<Track> {
        if (parent == OFFLINE) return app.downloads.saved.value.also { lists[parent] = it }
        if (parent == RECENT) return LastQueue.load(app)?.tracks.orEmpty()
        if (parent == SEARCH) return lists[SEARCH].orEmpty()
        val tracks = when {
            parent == LATEST -> withServer {
                api.tracks(mapOf("status" to "released", "sort" to "published", "limit" to "30"))
            }
            parent.startsWith(STATUS_PREFIX) -> catalog().filter { it.status == parent.removePrefix(STATUS_PREFIX) }
            parent.startsWith("release:") -> {
                val id = parent.removePrefix("release:").toLongOrNull() ?: return emptyList()
                val (r, list) = withServer { api.release(id) }
                releaseCache[id] = r
                list
            }
            else -> return emptyList()
        }
        lists[parent] = tracks
        return tracks
    }

    suspend fun children(parent: String, page: Int, pageSize: Int): List<MediaItem> {
        if (parent == ROOT) return rootChildren()
        if (parent == RECENT) return recentChildren()
        if (parent == RELEASES) return releases().paged(page, pageSize)
        if (parent == LIBRARY) return libraryFolders().paged(page, pageSize)
        return tracksOf(parent).map { trackItem(it, "$parent/track:${it.id}") }.paged(page, pageSize)
    }

    suspend fun search(query: String): List<Track> {
        val q = query.trim().lowercase()
        val found = try {
            withServer { api.tracks(mapOf("q" to query.trim(), "limit" to "50")) }
        } catch (e: Exception) {
            // Sans ton PC : on cherche dans ce qui est sur le téléphone
            app.downloads.saved.value.filter { q.isBlank() || it.title.lowercase().contains(q) || it.artist.lowercase().contains(q) }
        }
        lists[SEARCH] = found
        return found
    }

    fun searchItems(): List<MediaItem> = lists[SEARCH].orEmpty().map { trackItem(it, "$SEARCH/track:${it.id}") }

    suspend fun item(mediaId: String): MediaItem? {
        if (mediaId == ROOT) return root()
        if (mediaId == RECENT) return recentRoot()
        rootChildren().firstOrNull { it.mediaId == mediaId }?.let { return it }
        if (mediaId.startsWith(STATUS_PREFIX) && '/' !in mediaId)
            return libraryFolders().firstOrNull { it.mediaId == mediaId }
        if (mediaId.startsWith("release:") && '/' !in mediaId) {
            val id = mediaId.removePrefix("release:").toLongOrNull() ?: return null
            val r = releaseCache[id] ?: runCatching { withServer { api.release(id) }.first }.getOrNull() ?: return null
            return releaseItem(r)
        }
        val (parent, trackId) = parseTrackId(mediaId) ?: return null
        return findTrack(parent, trackId)?.let { trackItem(it, mediaId) }
    }

    // ------------------------------------------------------------ lecture

    /**
     * Ce que la voiture envoie (un identifiant) devient un morceau jouable.
     * Si on lance un seul titre d'un dossier, toute la liste du dossier part en file d'attente.
     */
    suspend fun resolveQueue(items: List<MediaItem>, startIndex: Int): Pair<List<MediaItem>, Int> {
        if (items.size == 1 && items[0].localConfiguration == null) {
            val parsed = parseTrackId(items[0].mediaId)
            if (parsed != null && parsed.first != null && parsed.first != RECENT) {
                val (parent, trackId) = parsed
                val list = lists[parent] ?: runCatching { tracksOf(parent!!) }.getOrDefault(emptyList())
                val index = list.indexOfFirst { it.id == trackId }
                if (index >= 0) return list.map { playable(it, "$parent/track:${it.id}") } to index
            }
            if (parsed?.first == RECENT) {
                LastQueue.load(app)?.let { q ->
                    return q.tracks.map { playable(it, "$RECENT/track:${it.id}") } to q.index.coerceIn(0, q.tracks.size - 1)
                }
            }
        }
        // Sans adresse audio, le lecteur planterait : ce qu'on ne reconnaît pas est écarté
        val ready = items.map { resolve(it) }.filter { it.localConfiguration != null }
        return ready to startIndex.coerceAtMost(ready.lastIndex)
    }

    suspend fun resolve(item: MediaItem): MediaItem {
        if (item.localConfiguration != null) return item
        val (parent, trackId) = parseTrackId(item.mediaId) ?: return item
        val t = findTrack(parent, trackId)
        if (t != null) return playable(t, item.mediaId)
        // Morceau inconnu ici : on le joue quand même avec ce que la voiture a envoyé
        val local = app.downloads.localFile(trackId)
        return item.buildUpon()
            .setUri(local?.let { Uri.fromFile(it) } ?: Uri.parse(api.audioUrl(trackId, app.streamingQuality())))
            .setMediaMetadata(item.mediaMetadata.buildUpon()
                .setArtworkUri(ArtworkProvider.trackUri(trackId)).setIsPlayable(true).setIsBrowsable(false).build())
            .build()
    }

    fun playable(t: Track, mediaId: String): MediaItem = trackItem(t, mediaId, withUri = true)

    private suspend fun findTrack(parent: String?, id: Long): Track? {
        parent?.let { p -> lists[p]?.firstOrNull { it.id == id } }?.let { return it }
        lists.values.firstNotNullOfOrNull { l -> l.firstOrNull { it.id == id } }?.let { return it }
        app.downloads.saved.value.firstOrNull { it.id == id }?.let { return it }
        LastQueue.load(app)?.tracks?.firstOrNull { it.id == id }?.let { return it }
        if (parent != null && parent != SEARCH && parent != RECENT)
            runCatching { tracksOf(parent) }.getOrNull()?.firstOrNull { it.id == id }?.let { return it }
        return null
    }

    // ------------------------------------------------------------ construction des éléments

    private fun folder(
        id: String, title: String, icon: String?, type: Int, browsableStyle: Int? = null, subtitle: String? = null,
    ): MediaItem {
        val extras = Bundle().apply {
            putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM)
            browsableStyle?.let { putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, it) }
        }
        return MediaItem.Builder()
            .setMediaId(id)
            .setMediaMetadata(MediaMetadata.Builder()
                .setTitle(title)
                .setSubtitle(subtitle)
                .setIsBrowsable(true)
                .setIsPlayable(false)
                .setMediaType(type)
                .setArtworkUri(icon?.let { Uri.parse("android.resource://${app.packageName}/drawable/$it") })
                .setExtras(extras)
                .build())
            .build()
    }

    private fun releaseItem(r: Release): MediaItem = MediaItem.Builder()
        .setMediaId("release:${r.id}")
        .setMediaMetadata(MediaMetadata.Builder()
            .setTitle(r.title)
            .setSubtitle(releaseSubtitle(r))
            .setAlbumTitle(r.title)
            .setIsBrowsable(true)
            .setIsPlayable(false)
            .setMediaType(MediaMetadata.MEDIA_TYPE_ALBUM)
            .setArtworkUri(ArtworkProvider.releaseUri(r.id))
            .setExtras(Bundle().apply {
                putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM)
            })
            .build())
        .build()

    fun trackItem(t: Track, mediaId: String, withUri: Boolean = false): MediaItem =
        buildTrackItem(app, t, mediaId, withUri)

    /** Élément affiché quand ton PC ne répond pas (non jouable) */
    fun unreachable(): MediaItem = MediaItem.Builder()
        .setMediaId(UNREACHABLE)
        .setMediaMetadata(MediaMetadata.Builder()
            .setTitle(if (app.settings.server.value.isBlank()) "L'appli n'est pas encore reliée" else "Ton PC ne répond pas")
            .setSubtitle(if (app.settings.server.value.isBlank()) "Ouvre Otoko Vault sur le téléphone pour la relier à ton PC"
                else "Vérifie Tailscale sur le téléphone · « Hors ligne » reste disponible")
            .setIsBrowsable(false)
            .setIsPlayable(false)
            .setArtworkUri(Uri.parse("android.resource://${app.packageName}/drawable/ic_auto_hors_ligne"))
            .build())
        .build()

    /** Si ton PC ne répond pas, on cherche une autre adresse (HTTPS, nom, IP Tailscale) puis on réessaie */
    private suspend fun <T> withServer(block: suspend () -> T): T = try {
        block()
    } catch (e: Exception) {
        if (!app.findServer()) throw e
        block()
    }

    companion object {
        const val ROOT = "root"
        const val RECENT = "recent"
        const val RELEASES = "releases"
        const val LATEST = "latest"
        const val LIBRARY = "library"
        const val OFFLINE = "offline"
        const val SEARCH = "search"
        const val UNREACHABLE = "unreachable"
        private const val STATUS_PREFIX = "$LIBRARY:"
        private const val CATALOG_TTL = 5 * 60 * 1000L
        private val STATUS_ORDER = listOf("released", "done", "wip", "idea")
        private val STATUS_TITLES = mapOf(
            "released" to "Publiés", "done" to "Terminés", "wip" to "En cours", "idea" to "Idées")

        /** « release:12/track:345 » → (« release:12 », 345) ; « track:345 » ou « 345 » → (null, 345) */
        fun parseTrackId(mediaId: String): Pair<String?, Long>? {
            val slash = mediaId.lastIndexOf('/')
            val parent = if (slash > 0) mediaId.substring(0, slash) else null
            val id = mediaId.substring(slash + 1).removePrefix("track:").toLongOrNull() ?: return null
            return parent to id
        }

        private val FR_DATE = DateTimeFormatter.ofPattern("d MMMM yyyy", Locale.FRENCH)

        /** « EP · 2026 » ou « Sort le 21 novembre 2026 » */
        fun releaseSubtitle(r: Release): String {
            val date = runCatching { LocalDate.parse(r.date.take(10)) }.getOrNull()
            if (r.upcoming && date != null) return "Sort le " + date.format(FR_DATE)
            return listOfNotNull(r.typeLabel.ifBlank { null }, date?.year?.toString()).joinToString(" · ")
        }
    }
}

private fun List<MediaItem>.paged(page: Int, pageSize: Int): List<MediaItem> {
    if (pageSize <= 0 || pageSize == Int.MAX_VALUE) return this
    val from = page.coerceAtLeast(0).toLong() * pageSize
    if (from >= size) return emptyList()
    return subList(from.toInt(), minOf(size, (from + pageSize).toInt()))
}

/** Un morceau en MediaItem : mêmes règles de qualité que l'appli, fichier local s'il est téléchargé */
fun buildTrackItem(app: App, t: Track, mediaId: String, withUri: Boolean): MediaItem {
    val b = MediaItem.Builder().setMediaId(mediaId)
    if (withUri) {
        val local = app.downloads.localFile(t.id)
        b.setUri(local?.let { Uri.fromFile(it) } ?: Uri.parse(app.api.audioUrl(t.id, app.streamingQuality())))
    }
    return b.setMediaMetadata(MediaMetadata.Builder()
        .setTitle(t.title)
        .setArtist(t.artist.ifBlank { null })
        .setAlbumTitle(t.release?.title)
        .setSubtitle(listOfNotNull(t.artist.ifBlank { null }, t.release?.title).joinToString(" · "))
        .setIsBrowsable(false)
        .setIsPlayable(true)
        .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
        .setDurationMs(if (t.duration > 0) (t.duration * 1000).toLong() else null)
        .setArtworkUri(ArtworkProvider.trackUri(t.id))
        // Le morceau complet ne voyage qu'avec ce qui se joue : dans une liste il triplerait le poids de chaque ligne
        .setExtras(if (withUri) Bundle().apply { putString(TRACK_JSON, t.toJson().toString()) } else null)
        .build())
        .build()
}

const val TRACK_JSON = "otoko.track"

/** La dernière file d'attente, gardée sur le téléphone pour reprendre la lecture */
class LastQueue(val tracks: List<Track>, val index: Int, val positionMs: Long) {
    companion object {
        private fun file(app: App) = File(app.filesDir, "derniere_file.json")

        fun save(app: App, tracks: List<Track>, index: Int, positionMs: Long) {
            if (tracks.isEmpty()) return
            // Au plus 200 morceaux autour du titre en cours
            val from = (index - 50).coerceAtLeast(0)
            val kept = tracks.subList(from, minOf(tracks.size, from + 200))
            val o = JSONObject().apply {
                put("index", index - from)
                put("position", positionMs)
                put("tracks", JSONArray().apply { kept.forEach { put(it.toJson()) } })
            }
            runCatching {
                val tmp = File(app.filesDir, "derniere_file.tmp")
                tmp.writeText(o.toString())
                tmp.renameTo(file(app))
            }
        }

        fun load(app: App): LastQueue? = runCatching {
            val o = JSONObject(file(app).readText())
            val a = o.getJSONArray("tracks")
            val tracks = List(a.length()) { Track.fromJson(a.getJSONObject(it)) }
            if (tracks.isEmpty()) null
            else LastQueue(tracks, o.optInt("index", 0).coerceIn(0, tracks.size - 1), o.optLong("position", 0))
        }.getOrNull()
    }
}
