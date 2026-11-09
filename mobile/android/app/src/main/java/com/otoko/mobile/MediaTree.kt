package com.otoko.mobile

import android.net.Uri
import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.session.MediaConstants
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import kotlin.random.Random

/**
 * L'arborescence vue par Android Auto (et tout autre navigateur de médias) :
 *   racine ─ Sorties ─ une sortie ─ ses morceaux
 *          ├ Derniers publiés
 *          ├ Bibliothèque ─ Publiés, Terminés, En cours, Idées ─ leurs morceaux
 *          └ Hors ligne ─ une sortie (ou « Sans sortie ») ─ ses morceaux gardés sur le téléphone
 * Identifiants : « releases », « release:12 », « latest », « library », « library:done », « offline »,
 * « offline:release:12 », « offline:none », et pour un morceau « <dossier>/track:345 » (le dossier sert à
 * remplir la file d'attente). Chaque liste de morceaux commence par « <dossier>/shuffle » : lecture aléatoire.
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

    /** Une liste venue de ton PC, gardée 5 minutes, et conservée s'il ne répond plus */
    private class Kept<T>(@Volatile var value: T? = null, @Volatile var at: Long = 0L)
    private val releasesKept = Kept<List<Release>>()
    private val latestKept = Kept<List<Track>>()

    private suspend fun <T> kept(k: Kept<T>, load: suspend () -> T): T {
        val now = System.currentTimeMillis()
        k.value?.let { if (now - k.at < CATALOG_TTL) return it }
        return try {
            withServer { load() }.also { k.value = it; k.at = now }
        } catch (e: Exception) {
            k.value ?: throw e
        }
    }

    // ------------------------------------------------------------ dossiers

    fun root(): MediaItem = folder(ROOT, "Otoko Vault", null, MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)

    fun rootChildren(): List<MediaItem> = listOf(
        folder(RELEASES, "Sorties", "ic_auto_sorties", MediaMetadata.MEDIA_TYPE_FOLDER_ALBUMS,
            browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM),
        folder(LATEST, "Derniers publiés", "ic_auto_nouveautes", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED),
        folder(LIBRARY, "Bibliothèque", "ic_auto_bibliotheque", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED,
            browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM),
        folder(OFFLINE, "Hors ligne", "ic_auto_hors_ligne", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED,
            browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM),
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
        val list = kept(releasesKept) { api.releases() }
        list.forEach { releaseCache[it.id] = it }
        return list.map { releaseItem(it) }
    }

    /** Hors ligne : la lecture aléatoire, puis une entrée par sortie (voir offlineGroups) */
    private fun offlineFolders(): List<MediaItem> {
        val saved = app.downloads.saved.value
        if (saved.isEmpty()) return listOf(notice(
            "Rien sur le téléphone pour l'instant",
            "Dans l'appli, touche la flèche d'un morceau pour le garder ici"))
        val groups = offlineGroups(saved)
        lists[OFFLINE] = groups.allTracks()
        return buildList {
            if (saved.size > 1) add(shuffleItem(OFFLINE, saved.size))
            groups.forEach { add(offlineFolder(it)) }
        }
    }

    private fun offlineFolder(g: OfflineGroup): MediaItem = MediaItem.Builder()
        .setMediaId(OFFLINE_PREFIX + g.key)
        .setMediaMetadata(MediaMetadata.Builder()
            .setTitle(g.title)
            .setSubtitle(g.subtitle)
            .setAlbumTitle(g.release?.title)
            .setIsBrowsable(true)
            .setIsPlayable(false)
            .setMediaType(MediaMetadata.MEDIA_TYPE_ALBUM)
            .setArtworkUri(g.release?.let { ArtworkProvider.releaseUri(it.id) } ?: ArtworkProvider.defaultUri)
            .setExtras(Bundle().apply {
                putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM)
            })
            .build())
        .build()

    /** En tête d'une liste de morceaux : tout le dossier, au hasard */
    private fun shuffleItem(parent: String, count: Int): MediaItem = MediaItem.Builder()
        .setMediaId("$parent/$SHUFFLE")
        .setMediaMetadata(MediaMetadata.Builder()
            .setTitle("Lecture aléatoire")
            .setSubtitle(morceaux(count))
            .setIsBrowsable(false)
            .setIsPlayable(true)
            .setMediaType(MediaMetadata.MEDIA_TYPE_PLAYLIST)
            .setArtworkUri(ArtworkProvider.iconUri("aleatoire"))
            .build())
        .build()

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
        if (parent == OFFLINE) return offlineGroups(app.downloads.saved.value).allTracks().also { lists[parent] = it }
        if (parent.startsWith(OFFLINE_PREFIX)) {
            val key = parent.removePrefix(OFFLINE_PREFIX)
            return offlineGroups(app.downloads.saved.value).firstOrNull { it.key == key }?.tracks.orEmpty()
                .also { lists[parent] = it }
        }
        if (parent == RECENT) return LastQueue.load(app)?.tracks.orEmpty()
        if (parent == SEARCH) return lists[SEARCH].orEmpty()
        val tracks = when {
            parent == LATEST -> kept(latestKept) {
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
        if (parent == OFFLINE) return offlineFolders().paged(page, pageSize)
        val tracks = tracksOf(parent)
        val items = tracks.map { trackItem(it, "$parent/track:${it.id}") }
        val withShuffle = if (tracks.size > 1 && parent != SEARCH) listOf(shuffleItem(parent, tracks.size)) + items else items
        return withShuffle.paged(page, pageSize)
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
        if (mediaId.startsWith(OFFLINE_PREFIX) && '/' !in mediaId) {
            val key = mediaId.removePrefix(OFFLINE_PREFIX)
            return offlineGroups(app.downloads.saved.value).firstOrNull { it.key == key }?.let { offlineFolder(it) }
        }
        if (mediaId.endsWith("/$SHUFFLE")) {
            val parent = mediaId.removeSuffix("/$SHUFFLE")
            return shuffleItem(parent, runCatching { tracksOf(parent) }.getOrDefault(emptyList()).size)
        }
        if (mediaId.startsWith("release:") && '/' !in mediaId) {
            val id = mediaId.removePrefix("release:").toLongOrNull() ?: return null
            val r = releaseCache[id] ?: runCatching { withServer { api.release(id) }.first }.getOrNull() ?: return null
            return releaseItem(r)
        }
        val (parent, trackId) = parseTrackId(mediaId) ?: return null
        return findTrack(parent, trackId)?.let { trackItem(it, mediaId) }
    }

    // ------------------------------------------------------------ lecture

    /** Une file prête à jouer ; shuffle = true pour la lecture aléatoire d'un dossier */
    class Queue(val items: List<MediaItem>, val index: Int, val shuffle: Boolean = false)

    /**
     * Ce que la voiture envoie (un identifiant) devient un morceau jouable.
     * Si on lance un seul titre d'un dossier, toute la liste du dossier part en file d'attente ;
     * « Lecture aléatoire » lance tout le dossier au hasard.
     */
    suspend fun resolveQueue(items: List<MediaItem>, startIndex: Int): Queue {
        if (items.size == 1 && items[0].mediaId.endsWith("/$SHUFFLE")) {
            val parent = items[0].mediaId.removeSuffix("/$SHUFFLE")
            val list = runCatching { tracksOf(parent) }.getOrDefault(emptyList())
            if (list.isNotEmpty())
                return Queue(list.map { playable(it, "$parent/track:${it.id}") }, Random.nextInt(list.size), shuffle = true)
        }
        if (items.size == 1 && items[0].localConfiguration == null) {
            val parsed = parseTrackId(items[0].mediaId)
            if (parsed != null && parsed.first != null && parsed.first != RECENT) {
                val (parent, trackId) = parsed
                val list = lists[parent] ?: runCatching { tracksOf(parent!!) }.getOrDefault(emptyList())
                val index = list.indexOfFirst { it.id == trackId }
                if (index >= 0) return Queue(list.map { playable(it, "$parent/track:${it.id}") }, index)
            }
            if (parsed?.first == RECENT) {
                LastQueue.load(app)?.let { q ->
                    return Queue(q.tracks.map { playable(it, "$RECENT/track:${it.id}") }, q.index.coerceIn(0, q.tracks.size - 1))
                }
            }
        }
        // Sans adresse audio, le lecteur planterait : ce qu'on ne reconnaît pas est écarté
        val ready = items.map { resolve(it) }.filter { it.localConfiguration != null }
        return Queue(ready, startIndex.coerceAtMost(ready.lastIndex))
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
    fun unreachable(): MediaItem =
        if (app.settings.server.value.isBlank())
            notice("L'appli n'est pas encore reliée", "Ouvre Otoko Vault sur le téléphone pour la relier à ton PC")
        else notice("Ton PC ne répond pas", "Vérifie que son serveur tourne et que Tailscale est actif · « Hors ligne » reste disponible")

    /** Une ligne d'information, ni jouable ni dossier */
    private fun notice(title: String, subtitle: String): MediaItem = MediaItem.Builder()
        .setMediaId(UNREACHABLE)
        .setMediaMetadata(MediaMetadata.Builder()
            .setTitle(title)
            .setSubtitle(subtitle)
            .setIsBrowsable(false)
            .setIsPlayable(false)
            .setArtworkUri(ArtworkProvider.iconUri("hors_ligne"))
            .build())
        .build()

    /**
     * Si ton PC ne répond pas, on cherche une autre adresse (HTTPS, nom, IP Tailscale) puis on réessaie.
     * Il vient de ne répondre nulle part : on le dit tout de suite au lieu de refaire attendre la voiture.
     */
    private suspend fun <T> withServer(block: suspend () -> T): T {
        if (app.serverDown()) throw IOException("Ton PC ne répond pas")
        return try {
            block()
        } catch (e: Exception) {
            if (!app.findServer()) throw e
            block()
        }
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
        const val SHUFFLE = "shuffle"
        private const val STATUS_PREFIX = "$LIBRARY:"
        private const val OFFLINE_PREFIX = "$OFFLINE:"
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
