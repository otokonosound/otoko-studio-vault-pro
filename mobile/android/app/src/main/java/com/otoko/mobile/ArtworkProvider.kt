package com.otoko.mobile

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import java.io.File
import java.io.FileNotFoundException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.ConcurrentHashMap

/**
 * Pochettes pour Android Auto, qui charge mal les adresses http(s) :
 * content://com.otoko.mobile.artwork/release/12 ou /track/345, téléchargées une fois dans le cache.
 */
class ArtworkProvider : ContentProvider() {

    override fun onCreate() = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        if (mode != "r") throw SecurityException("Lecture seule")
        val (kind, id) = uri.pathSegments.takeIf { it.size == 2 }?.let { it[0] to it[1].toLongOrNull() }
            ?: throw FileNotFoundException(uri.toString())
        if (id == null || kind !in setOf("release", "track")) throw FileNotFoundException(uri.toString())
        val dir = File(context!!.cacheDir, "pochettes").apply { mkdirs() }
        val file = File(dir, "$kind-$id.jpg")
        val now = System.currentTimeMillis()
        // Une semaine en cache : les pochettes changent rarement
        if (!file.exists() || now - file.lastModified() > 7L * 24 * 3600 * 1000) {
            // Pas de pochette (la plupart des idées) ou PC injoignable : on ne redemande pas à chaque ligne affichée
            if (now - (missing["$kind-$id"] ?: 0L) > MISSING_TTL) {
                val api = App.instance.api
                val url = if (kind == "release") api.releaseCoverUrl(id, SIZE) else api.coverUrl(id, SIZE)
                runCatching {
                    val c = URL(url).openConnection() as HttpURLConnection
                    c.connectTimeout = 8000
                    c.readTimeout = 15000
                    try {
                        if (c.responseCode != 200) error("Pochette absente")
                        val part = File(dir, "$kind-$id.part")
                        c.inputStream.use { input -> part.outputStream().use { input.copyTo(it) } }
                        part.renameTo(file)
                    } finally {
                        c.disconnect()
                    }
                }.onFailure { missing["$kind-$id"] = now }
            }
            if (!file.exists()) throw FileNotFoundException("Pochette indisponible")
        }
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    private val missing = ConcurrentHashMap<String, Long>()

    override fun getType(uri: Uri) = "image/jpeg"
    override fun query(uri: Uri, p: Array<out String>?, s: String?, a: Array<out String>?, o: String?): Cursor? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, s: String?, a: Array<out String>?) = 0
    override fun update(uri: Uri, v: ContentValues?, s: String?, a: Array<out String>?) = 0

    companion object {
        const val AUTHORITY = "com.otoko.mobile.artwork"
        private const val SIZE = 300
        private const val MISSING_TTL = 10 * 60 * 1000L
        fun trackUri(id: Long): Uri = Uri.parse("content://$AUTHORITY/track/$id")
        fun releaseUri(id: Long): Uri = Uri.parse("content://$AUTHORITY/release/$id")
    }
}
