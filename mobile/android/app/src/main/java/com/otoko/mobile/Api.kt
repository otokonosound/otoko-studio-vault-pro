package com.otoko.mobile

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder

data class Track(
    val id: Long,
    val title: String,
    val artist: String,
    val duration: Double,
    val bpm: Double?,
    val key: String,
    val camelot: String,
    val lufs: Double?,
    val moods: List<String>,
    val nature: String,
    val natureLabel: String,
    val status: String,
    val statusLabel: String,
    val format: String,
    val sampleRate: Int,
    val size: Long,
    val release: ReleaseRef?,
    val color: Int?,
    /** vraie date de publication (AAAA-MM-JJ), vide si inconnue */
    val publishedDate: String = "",
    /** 1 gardé, -1 rejeté, 0 rien */
    val pick: Int = 0,
) {
    /** "Original, hypnotique" comme dans la maquette */
    val subtitle: String get() = listOfNotNull(natureLabel.ifBlank { null }, moods.firstOrNull()).joinToString(", ")
    val qualityLabel: String
        get() = buildString {
            append("Sans perte")
            if (sampleRate > 0) {
                val khz = if (sampleRate % 1000 == 0) "${sampleRate / 1000}" else "%.1f".format(sampleRate / 1000.0)
                append(" · $khz kHz")
            }
        }

    fun toJson(): JSONObject = JSONObject().apply {
        put("id", id); put("title", title); put("artist", artist); put("duration", duration)
        put("bpm", bpm ?: JSONObject.NULL); put("key", key); put("camelot", camelot)
        put("lufs", lufs ?: JSONObject.NULL); put("moods", JSONArray(moods))
        put("nature", nature); put("nature_label", natureLabel); put("status", status)
        put("status_label", statusLabel); put("format", format); put("samplerate", sampleRate)
        put("size", size); put("release_title", release?.title ?: JSONObject.NULL)
        put("release", release?.toJson() ?: JSONObject.NULL)
        put("published_date", publishedDate)
        put("pick", pick)
        put("color", color?.let { String.format("#%06X", it and 0xFFFFFF) } ?: JSONObject.NULL)
    }

    companion object {
        fun fromJson(o: JSONObject): Track {
            val moods = o.optJSONArray("moods")?.let { a -> List(a.length()) { a.getString(it) } } ?: emptyList()
            // Les anciens téléchargements n'ont que le titre de la sortie
            val release = o.optJSONObject("release")?.let { ReleaseRef.fromJson(it) }
                ?: o.optStr("release_title")?.let { ReleaseRef(0, it, "", "", "") }
            return Track(
                id = o.getLong("id"),
                title = o.optStr("title") ?: "Sans titre",
                artist = o.optStr("artist") ?: "",
                duration = o.optDouble("duration", 0.0).takeUnless { it.isNaN() } ?: 0.0,
                bpm = o.optNum("bpm"),
                key = o.optStr("key") ?: "",
                camelot = o.optStr("camelot") ?: "",
                lufs = o.optNum("lufs"),
                moods = moods,
                nature = o.optStr("nature") ?: "",
                natureLabel = o.optStr("nature_label") ?: "",
                status = o.optStr("status") ?: "",
                statusLabel = o.optStr("status_label") ?: "",
                format = o.optStr("format") ?: "",
                sampleRate = o.optInt("samplerate", 0),
                size = o.optLong("size", 0),
                release = release,
                color = o.optStr("color")?.let { parseColor(it) },
                publishedDate = o.optStr("published_date") ?: "",
                pick = o.optInt("pick", 0),
            )
        }
    }
}

/** La sortie d'un morceau, en bref (id 0 : inconnue, vieux téléchargement) */
data class ReleaseRef(
    val id: Long, val title: String, val type: String, val typeLabel: String, val date: String,
    /** Place du morceau dans la sortie (0 si inconnue) */
    val position: Int = 0,
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("id", id); put("title", title); put("type", type); put("type_label", typeLabel); put("date", date)
        put("position", position)
    }

    companion object {
        fun fromJson(o: JSONObject) = ReleaseRef(
            id = o.optLong("id", 0),
            title = o.optStr("title") ?: "Sans titre",
            type = o.optStr("type") ?: "",
            typeLabel = o.optStr("type_label") ?: "",
            date = o.optStr("date") ?: "",
            position = o.optInt("position", 0),
        )
    }
}

/** Single, EP, album… publié (ou à venir) */
data class Release(
    val id: Long,
    val title: String,
    val type: String,
    val typeLabel: String,
    /** AAAA-MM-JJ */
    val date: String,
    val description: String,
    val soundcloud: String,
    val tracks: Int,
    val duration: Double,
    val upcoming: Boolean,
    val color: Int?,
) {
    companion object {
        fun fromJson(o: JSONObject) = Release(
            id = o.getLong("id"),
            title = o.optStr("title") ?: "Sans titre",
            type = o.optStr("type") ?: "other",
            typeLabel = o.optStr("type_label") ?: "",
            date = o.optStr("date") ?: "",
            description = o.optStr("description") ?: "",
            soundcloud = o.optStr("soundcloud") ?: "",
            tracks = o.optInt("tracks", 0),
            duration = o.optDouble("duration", 0.0).takeUnless { it.isNaN() } ?: 0.0,
            upcoming = o.optBoolean("upcoming", false),
            color = o.optStr("color")?.let { parseColor(it) },
        )
    }
}

data class Waveform(val peaks: List<Float>, val duration: Double)

/** Une note posée à un instant du morceau (un repère, le même que sur ton PC) */
data class Note(val time: Double, val label: String) {
    fun toJson(): JSONObject = JSONObject().put("time", time).put("label", label)
}

/** Ce que tu penses d'un morceau : gardé ou rejeté, son statut, tes notes */
data class Mine(val trackId: Long, val pick: Int, val status: String, val notes: List<Note>) {
    companion object {
        fun fromJson(o: JSONObject): Mine {
            val a = o.optJSONArray("markers") ?: JSONArray()
            return Mine(
                trackId = o.getLong("id"),
                pick = o.optInt("pick", 0),
                status = o.optStr("status") ?: "idea",
                notes = List(a.length()) { a.getJSONObject(it).let { n -> Note(n.optDouble("time", 0.0), n.optString("label")) } },
            )
        }
    }
}

/** Les statuts de ton PC, dans l'ordre du tableau de production */
val STATUSES = listOf(
    "idea" to "Idée", "draft" to "Brouillon", "wip" to "En cours", "to_mix" to "À mixer",
    "to_master" to "À masteriser", "done" to "Terminé", "released" to "Publié",
)

/** Lien de partage public (écoute privée pour quelqu'un sans Tailscale) */
data class Share(
    val id: Long,
    val label: String,
    val message: String,
    /** releases, tracks ou discography */
    val kind: String,
    val items: List<Long>,
    val canDownload: Boolean,
    /** original, lossless ou high */
    val downloadQuality: String,
    val maxDownloads: Int?,
    val downloads: Int,
    val views: Int,
    val plays: Int,
    val lastAccess: String?,
    val expiresAt: String?,
    val hasPassword: Boolean,
    /** active, expired ou revoked */
    val state: String,
    val url: String,
    /** Les retours de la personne qui écoute : « j'aime », avis, personnes à l'écoute, morceaux écoutés en entier */
    val likes: Int = 0,
    val comments: Int = 0,
    val listeners: Int = 0,
    val finished: Int = 0,
) {
    companion object {
        fun fromJson(o: JSONObject): Share {
            val a = o.optJSONArray("items") ?: JSONArray()
            return Share(
                id = o.getLong("id"),
                label = o.optStr("label") ?: "",
                message = o.optStr("message") ?: "",
                kind = o.optStr("kind") ?: "releases",
                items = List(a.length()) { a.optLong(it) },
                canDownload = o.optBoolean("can_download", false),
                downloadQuality = o.optStr("download_quality") ?: "original",
                maxDownloads = if (o.isNull("max_downloads")) null else o.optInt("max_downloads").takeIf { it > 0 },
                downloads = o.optInt("downloads", 0),
                views = o.optInt("views", 0),
                plays = o.optInt("plays", 0),
                lastAccess = o.optStr("last_access"),
                expiresAt = o.optStr("expires_at"),
                hasPassword = o.optBoolean("has_password", false),
                state = o.optStr("state") ?: "active",
                url = o.optStr("url") ?: "",
                likes = o.optInt("likes", 0),
                comments = o.optInt("comments", 0),
                listeners = o.optInt("listeners", 0),
                finished = o.optInt("finished", 0),
            )
        }
    }
}

data class SharesInfo(val publicBase: String, val funnel: Boolean, val shares: List<Share>)

private fun JSONObject.optStr(k: String): String? = if (isNull(k)) null else optString(k).ifBlank { null }
private fun JSONObject.optNum(k: String): Double? = if (isNull(k)) null else optDouble(k).takeUnless { it.isNaN() }

fun parseColor(s: String): Int? = runCatching {
    val hex = s.removePrefix("#")
    if (hex.length != 6) null else (0xFF000000 or hex.toLong(16)).toInt()
}.getOrNull()

enum class Quality(val param: String, val label: String, val hint: String) {
    // FLAC sans perte préparé par ton PC : le lecteur d'Android fait saturer les WAV 32 bits flottants
    ORIGINAL("lossless", "Origine", "Sans perte (FLAC 24 bits), environ 50 Mo par morceau."),
    HIGH("high", "Haute", "Opus 256 kb/s : transparent à l'oreille, environ 8 Mo par morceau."),
    ECO("eco", "Éco", "Opus 128 kb/s : très bon son, environ 4 Mo par morceau."),
}

/** Accès au serveur de ton PC (via Tailscale). */
class Api(
    /** Les partages se gèrent seulement par HTTPS Tailscale : ton PC les refuse depuis le Wi-Fi de la maison */
    private val shareBase: () -> String = { App.instance.settings.httpsServer ?: App.instance.settings.server.value },
    private val baseUrl: () -> String,
) {

    fun url(path: String): String = baseUrl().trimEnd('/') + path
    fun coverUrl(id: Long, size: Int = 600) = url("/api/tracks/$id/cover?size=$size")
    fun audioUrl(id: Long, q: Quality) = url("/api/tracks/$id/audio?quality=${q.param}")

    suspend fun tracks(params: Map<String, String>): List<Track> = withContext(Dispatchers.IO) {
        val qs = params.filterValues { it.isNotEmpty() }
            .map { (k, v) -> "$k=" + URLEncoder.encode(v, "UTF-8") }.joinToString("&")
        val arr = JSONArray(get("/api/tracks?$qs"))
        List(arr.length()) { Track.fromJson(arr.getJSONObject(it)) }
    }

    suspend fun waveform(id: Long, points: Int = 64): Waveform = withContext(Dispatchers.IO) {
        val o = JSONObject(get("/api/tracks/$id/waveform?points=$points"))
        val a = o.getJSONArray("peaks")
        Waveform(List(a.length()) { a.getDouble(it).toFloat() }, o.optDouble("duration", 0.0))
    }

    fun releaseCoverUrl(id: Long, size: Int = 600) = url("/api/releases/$id/cover?size=$size")

    suspend fun releases(): List<Release> = withContext(Dispatchers.IO) {
        val arr = JSONArray(get("/api/releases"))
        List(arr.length()) { Release.fromJson(arr.getJSONObject(it)) }
    }

    /** La sortie et ses morceaux, dans l'ordre */
    suspend fun release(id: Long): Pair<Release, List<Track>> = withContext(Dispatchers.IO) {
        val o = JSONObject(get("/api/releases/$id"))
        val arr = o.optJSONArray("track_list") ?: JSONArray()
        Release.fromJson(o) to List(arr.length()) { Track.fromJson(arr.getJSONObject(it)) }
    }

    /** Le morceau avec ses notes, son statut et ton choix (gardé, rejeté) */
    suspend fun mine(id: Long): Mine = withContext(Dispatchers.IO) { Mine.fromJson(JSONObject(get("/api/tracks/$id"))) }

    /** Une écriture sur ton PC (garder, statut, note, écoute) : voir Outbox, qui la garde si le réseau manque */
    suspend fun post(path: String, body: JSONObject): JSONObject = withContext(Dispatchers.IO) {
        JSONObject(request("POST", path, 15000, baseUrl(), body, mapOf("X-Studio-Vault" to "1")).ifBlank { "{}" })
    }

    suspend fun ping(): Boolean = pingInfo() != null

    /** Réponse de /api/ping, null si ton PC ne répond pas à cette adresse */
    suspend fun pingInfo(): JSONObject? = pingInfoAt(baseUrl())

    /** Pareil, à une adresse donnée (sans changer l'adresse utilisée par le reste de l'appli) */
    suspend fun pingInfoAt(base: String, timeout: Int = 4000): JSONObject? = withContext(Dispatchers.IO) {
        runCatching { JSONObject(get("/api/ping", timeout, base)).takeIf { it.has("tracks") } }.getOrNull()
    }

    /** Le numéro de la version d'Otoko Vault que ton PC propose, null s'il n'en a pas */
    suspend fun apkCode(): Long? = withContext(Dispatchers.IO) {
        runCatching { JSONObject(get("/api/maj", 6000)).let { if (it.isNull("apk_code")) null else it.getLong("apk_code") } }.getOrNull()
    }

    /** Pour enregistrer dans Musique : le fichier avec titre, artiste, sortie et pochette dedans */
    fun taggedAudioUrl(id: Long, q: Quality) = audioUrl(id, q) + "&tags=1"

    // ------------------------------------------------------------ partages (toujours par HTTPS Tailscale)

    suspend fun shares(): SharesInfo = withContext(Dispatchers.IO) {
        val o = JSONObject(shareCall("GET", "/api/shares"))
        val a = o.optJSONArray("shares") ?: JSONArray()
        SharesInfo(o.optString("public_base"), o.optBoolean("funnel", false), List(a.length()) { Share.fromJson(a.getJSONObject(it)) })
    }

    /** true si le partage public (Tailscale Funnel) est allumé sur ton PC */
    suspend fun sharesFunnel(): Boolean = withContext(Dispatchers.IO) {
        JSONObject(shareCall("GET", "/api/shares/status")).optBoolean("funnel", false)
    }

    suspend fun createShare(body: JSONObject): Share = withContext(Dispatchers.IO) {
        Share.fromJson(JSONObject(shareCall("POST", "/api/shares", body)))
    }

    suspend fun updateShare(id: Long, body: JSONObject): Share = withContext(Dispatchers.IO) {
        Share.fromJson(JSONObject(shareCall("POST", "/api/shares/$id", body)))
    }

    suspend fun deleteShare(id: Long) = withContext(Dispatchers.IO) { shareCall("DELETE", "/api/shares/$id") }

    private fun shareCall(method: String, path: String, body: JSONObject? = null): String = try {
        request(method, path, 15000, shareBase(), body, mapOf("X-Studio-Vault" to "1"))
    } catch (e: ServerError) {
        throw e
    } catch (e: Exception) {
        throw IOException("Les partages demandent Tailscale connecté.")
    }

    /** Ton PC a répondu, mais par une erreur (son message est lisible tel quel) */
    class ServerError(msg: String) : IOException(msg)

    private fun get(path: String, timeout: Int = 15000, base: String = baseUrl()): String =
        request("GET", path, timeout, base)

    private fun request(
        method: String, path: String, timeout: Int, base: String,
        body: JSONObject? = null, headers: Map<String, String> = emptyMap(),
    ): String {
        val c = URL(base.trimEnd('/') + path).openConnection() as HttpURLConnection
        c.connectTimeout = timeout
        c.readTimeout = timeout
        c.requestMethod = method
        headers.forEach { (k, v) -> c.setRequestProperty(k, v) }
        try {
            if (body != null) {
                val bytes = body.toString().toByteArray(Charsets.UTF_8)
                c.doOutput = true
                c.setRequestProperty("Content-Type", "application/json; charset=utf-8")
                c.setFixedLengthStreamingMode(bytes.size)
                c.outputStream.use { it.write(bytes) }
            }
            val code = c.responseCode
            val body = (if (code < 400) c.inputStream else c.errorStream)?.bufferedReader()?.readText() ?: ""
            if (code >= 400) {
                val msg = runCatching { JSONObject(body).getString("error") }.getOrNull()
                throw ServerError(msg ?: "Erreur $code du serveur")
            }
            return body
        } finally {
            c.disconnect()
        }
    }
}
