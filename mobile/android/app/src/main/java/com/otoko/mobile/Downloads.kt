package com.otoko.mobile

import android.Manifest
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL

sealed interface DlState {
    data class Running(val progress: Float) : DlState
    data object Done : DlState
    data class Failed(val message: String) : DlState
}

/**
 * Morceaux gardés sur le téléphone, en qualité d'origine, avec leurs tags (titre, artiste, sortie, pochette).
 * Si le réglage est actif, une copie va aussi dans Musique/OTOKO, visible des autres applis.
 */
class Downloads(
    private val context: Context,
    private val api: Api,
    private val settings: Settings,
    private val scope: CoroutineScope,
) {
    private val dir = File(context.filesDir, "hors_ligne").apply { mkdirs() }
    private val gate = Semaphore(2)
    private val jobs = mutableMapOf<Long, Job>()
    private val batchJobs = mutableMapOf<Long, Job>()
    private val musicLock = Mutex()

    private val _states = MutableStateFlow<Map<Long, DlState>>(emptyMap())
    val states: StateFlow<Map<Long, DlState>> = _states
    private val _saved = MutableStateFlow<List<Track>>(emptyList())
    /** Morceaux téléchargés, avec leurs infos pour fonctionner sans réseau */
    val saved: StateFlow<List<Track>> = _saved
    private val _inMusic = MutableStateFlow<Set<Long>>(emptySet())
    /** Morceaux dont une copie a été enregistrée dans Musique */
    val inMusic: StateFlow<Set<Long>> = _inMusic
    private val _batches = MutableStateFlow<Map<Long, Pair<Int, Int>>>(emptyMap())
    /** Sorties en cours de téléchargement complet : id de la sortie → (titres traités, total) */
    val batches: StateFlow<Map<Long, Pair<Int, Int>>> = _batches

    /** Infos propres au fichier : extension réelle, tags présents, copie dans Musique */
    private val extras = mutableMapOf<Long, JSONObject>()

    init {
        val tracks = dir.listFiles { f -> f.name.endsWith(".json") }.orEmpty().mapNotNull { f ->
            runCatching {
                val o = JSONObject(f.readText())
                Track.fromJson(o).also { extras[it.id] = o }
            }.getOrNull()?.takeIf { audioFile(it.id).exists() }
        }.sortedByDescending { metaFile(it.id).lastModified() }
        _saved.value = tracks
        _states.value = tracks.associate { it.id to DlState.Done }
        _inMusic.value = tracks.filter { extras[it.id]?.optBoolean("in_music") == true }.map { it.id }.toSet()
    }

    private fun audioFile(id: Long) = File(dir, "$id.audio")
    private fun metaFile(id: Long) = File(dir, "$id.json")

    fun localFile(id: Long): File? = audioFile(id).takeIf { _states.value[id] == DlState.Done && it.exists() }
    fun usedBytes(): Long = dir.listFiles().orEmpty().sumOf { it.length() }

    private fun writeMeta(track: Track, ext: String, tagged: Boolean, inMusic: Boolean) {
        val o = track.toJson().apply {
            put("file_ext", ext); put("tagged", tagged); put("in_music", inMusic)
        }
        extras[track.id] = o
        metaFile(track.id).writeText(o.toString())
    }

    fun start(track: Track) {
        if (jobs[track.id]?.isActive == true || _states.value[track.id] == DlState.Done) return
        _states.update { it + (track.id to DlState.Running(0f)) }
        jobs[track.id] = scope.launch(Dispatchers.IO) {
            gate.withPermit {
                val part = File(dir, "${track.id}.part")
                try {
                    val c = URL(api.taggedAudioUrl(track.id, Quality.ORIGINAL)).openConnection() as HttpURLConnection
                    c.connectTimeout = 15000
                    c.readTimeout = 60000
                    if (c.responseCode != 200) error("Le serveur a répondu ${c.responseCode}")
                    val total = c.contentLengthLong.takeIf { it > 0 } ?: track.size
                    val typeExt = extFromMime(c.contentType)
                    var done = 0L
                    var lastShown = 0f
                    c.inputStream.use { input ->
                        part.outputStream().use { out ->
                            val buf = ByteArray(256 * 1024)
                            while (isActive) {
                                val n = input.read(buf)
                                if (n < 0) break
                                out.write(buf, 0, n)
                                done += n
                                val p = if (total > 0) done.toFloat() / total else 0f
                                if (p - lastShown >= 0.01f) {
                                    lastShown = p
                                    _states.update { it + (track.id to DlState.Running(p)) }
                                }
                            }
                        }
                    }
                    if (!isActive) { part.delete(); return@withPermit }
                    part.renameTo(audioFile(track.id))
                    val ext = typeExt ?: sniffExt(audioFile(track.id))
                    writeMeta(track, ext, tagged = true, inMusic = false)
                    _saved.update { listOf(track) + it.filter { t -> t.id != track.id } }
                    _states.update { it + (track.id to DlState.Done) }
                    if (settings.saveToMusic.value) saveToMusic(track)
                } catch (e: Exception) {
                    part.delete()
                    _states.update { it + (track.id to DlState.Failed(e.message ?: "Échec du téléchargement")) }
                }
            }
        }
    }

    /** Toute une sortie, un titre à la fois, en sautant ceux déjà sur le téléphone */
    fun startRelease(releaseId: Long, tracks: List<Track>) {
        if (batchJobs[releaseId]?.isActive == true) return
        val todo = tracks.filter { _states.value[it.id] != DlState.Done }
        if (todo.isEmpty()) return
        val total = tracks.size
        var done = total - todo.size
        _batches.update { it + (releaseId to (done to total)) }
        batchJobs[releaseId] = scope.launch {
            try {
                for (t in todo) {
                    start(t)
                    // attend la fin de ce titre (réussi, échoué ou retiré) avant de passer au suivant
                    states.first { s -> s[t.id].let { it == null || it == DlState.Done || it is DlState.Failed } }
                    done++
                    _batches.update { it + (releaseId to (done to total)) }
                }
            } finally {
                _batches.update { it - releaseId }
            }
        }
    }

    fun removeAll(tracks: List<Track>) = tracks.forEach { remove(it.id) }

    /** Retire du téléphone (stockage de l'appli). La copie dans Musique, elle, reste. */
    fun remove(id: Long) {
        jobs.remove(id)?.cancel()
        audioFile(id).delete()
        metaFile(id).delete()
        File(dir, "$id.part").delete()
        extras.remove(id)
        _saved.update { list -> list.filter { it.id != id } }
        _states.update { it - id }
        _inMusic.update { it - id }
    }

    // ------------------------------------------------------------ copie dans Musique

    /** Copie dans Musique/OTOKO. null si c'est fait, sinon la raison. */
    suspend fun saveToMusic(track: Track): String? = withContext(Dispatchers.IO) {
        musicLock.withLock {
            val src = localFile(track.id) ?: return@withLock "Ce morceau n'est pas encore sur le téléphone."
            var meta = extras[track.id]
            // Anciens téléchargements sans tags : on les remplace par la version avec tags si ton PC répond
            if (meta?.optBoolean("tagged") != true) refetchTagged(track)
            meta = extras[track.id]
            val ext = meta?.optString("file_ext")?.ifBlank { null } ?: sniffExt(src)
            val name = safeName(if (track.artist.isBlank()) track.title else "${track.artist} - ${track.title}") + ".$ext"
            val err = try {
                writeToMusic(name, mimeOf(ext), track) { src.inputStream() }
            } catch (e: Exception) {
                e.message ?: "Copie impossible."
            }
            if (err == null) {
                writeMeta(track, ext, meta?.optBoolean("tagged") == true, inMusic = true)
                _inMusic.update { it + track.id }
            }
            err
        }
    }

    /** Tout ce qui est hors ligne et pas encore dans Musique. Renvoie (réussis, échecs). */
    suspend fun saveAllToMusic(): Pair<Int, Int> {
        var ok = 0
        var ko = 0
        for (t in _saved.value) {
            if (t.id in _inMusic.value) continue
            if (saveToMusic(t) == null) ok++ else ko++
        }
        return ok to ko
    }

    private fun refetchTagged(track: Track) {
        val tmp = File(dir, "${track.id}.tags.part")
        runCatching {
            val c = URL(api.taggedAudioUrl(track.id, Quality.ORIGINAL)).openConnection() as HttpURLConnection
            c.connectTimeout = 6000
            c.readTimeout = 60000
            try {
                if (c.responseCode != 200) error("Le serveur a répondu ${c.responseCode}")
                val ext = extFromMime(c.contentType)
                c.inputStream.use { input -> tmp.outputStream().use { input.copyTo(it, 256 * 1024) } }
                if (tmp.length() > 0 && tmp.renameTo(audioFile(track.id)))
                    writeMeta(track, ext ?: sniffExt(audioFile(track.id)), tagged = true,
                        inMusic = extras[track.id]?.optBoolean("in_music") == true)
            } finally {
                c.disconnect()
            }
        }
        tmp.delete()
    }

    private fun writeToMusic(name: String, mime: String, track: Track, open: () -> InputStream): String? {
        if (Build.VERSION.SDK_INT >= 29) {
            val resolver = context.contentResolver
            val collection = MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            val relPath = Environment.DIRECTORY_MUSIC + "/OTOKO/"
            // Même nom déjà enregistré par l'appli : on le remplace au lieu de créer « (1) »
            val existing: Uri? = resolver.query(
                collection, arrayOf(MediaStore.Audio.Media._ID),
                "${MediaStore.Audio.Media.DISPLAY_NAME} = ? AND ${MediaStore.Audio.Media.RELATIVE_PATH} = ?",
                arrayOf(name, relPath), null,
            )?.use { c -> if (c.moveToFirst()) android.content.ContentUris.withAppendedId(collection, c.getLong(0)) else null }
            val values = ContentValues().apply {
                put(MediaStore.Audio.Media.DISPLAY_NAME, name)
                put(MediaStore.Audio.Media.MIME_TYPE, mime)
                put(MediaStore.Audio.Media.RELATIVE_PATH, relPath)
                put(MediaStore.Audio.Media.TITLE, track.title)
                put(MediaStore.Audio.Media.ARTIST, track.artist.ifBlank { null })
                track.release?.title?.let { put(MediaStore.Audio.Media.ALBUM, it) }
                put(MediaStore.Audio.Media.IS_MUSIC, 1)
                put(MediaStore.Audio.Media.IS_PENDING, 1)
            }
            val uri = existing?.also { resolver.update(it, values, null, null) }
                ?: resolver.insert(collection, values)
                ?: return "Android refuse d'enregistrer dans Musique."
            try {
                resolver.openOutputStream(uri, "wt")?.use { out -> open().use { it.copyTo(out, 256 * 1024) } }
                    ?: error("Écriture impossible")
            } catch (e: Exception) {
                if (existing == null) resolver.delete(uri, null, null)
                throw e
            }
            resolver.update(uri, ContentValues().apply { put(MediaStore.Audio.Media.IS_PENDING, 0) }, null, null)
            return null
        }
        // Android 9 et avant : écriture directe dans le dossier Musique (permission de stockage)
        if (context.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED)
            return "Autorise l'accès au stockage dans les réglages d'Android pour enregistrer dans Musique."
        @Suppress("DEPRECATION")
        val target = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC), "OTOKO")
            .apply { mkdirs() }
        val file = File(target, name)
        open().use { input -> file.outputStream().use { input.copyTo(it, 256 * 1024) } }
        MediaScannerConnection.scanFile(context, arrayOf(file.path), arrayOf(mime), null)
        return null
    }

    companion object {
        private val FORBIDDEN = "<>:\"/\\|?*"

        fun safeName(s: String): String =
            s.map { if (it in FORBIDDEN || it.code < 32) '_' else it }.joinToString("").trim(' ', '.').take(120)
                .ifBlank { "Morceau" }

        fun extFromMime(type: String?): String? = when (type?.substringBefore(';')?.trim()?.lowercase()) {
            "audio/flac", "audio/x-flac" -> "flac"
            "audio/mpeg" -> "mp3"
            "audio/wav", "audio/x-wav", "audio/wave" -> "wav"
            "audio/ogg", "audio/opus" -> "ogg"
            "audio/mp4", "audio/m4a", "audio/x-m4a" -> "m4a"
            "audio/aiff", "audio/x-aiff" -> "aiff"
            "audio/aac" -> "aac"
            else -> null
        }

        fun mimeOf(ext: String): String = when (ext) {
            "flac" -> "audio/flac"
            "mp3" -> "audio/mpeg"
            "wav" -> "audio/wav"
            "ogg" -> "audio/ogg"
            "m4a" -> "audio/mp4"
            "aiff" -> "audio/aiff"
            "aac" -> "audio/aac"
            else -> "audio/*"
        }

        /** Format d'un fichier d'après ses premiers octets (anciens téléchargements) */
        fun sniffExt(f: File): String {
            val h = ByteArray(12)
            val n = runCatching { f.inputStream().use { it.read(h) } }.getOrDefault(0)
            if (n < 4) return "flac"
            val s = String(h, 0, n, Charsets.ISO_8859_1)
            return when {
                s.startsWith("fLaC") -> "flac"
                s.startsWith("ID3") || (h[0] == 0xFF.toByte() && (h[1].toInt() and 0xE0) == 0xE0) -> "mp3"
                s.startsWith("RIFF") -> "wav"
                s.startsWith("OggS") -> "ogg"
                s.startsWith("FORM") -> "aiff"
                n >= 8 && s.substring(4, 8) == "ftyp" -> "m4a"
                else -> "flac"
            }
        }
    }
}
