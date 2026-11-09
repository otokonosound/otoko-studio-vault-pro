package com.otoko.mobile

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.net.Uri
import android.os.ParcelFileDescriptor
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.ConcurrentHashMap

/**
 * Pochettes pour Android Auto, qui charge mal les adresses http(s) :
 * content://com.otoko.mobile.artwork/release/12, /track/345 ou /defaut.
 *
 * Dans l'ordre : la pochette gardée hors ligne avec le morceau, celle déjà en cache, celle de ton PC s'il répond,
 * sinon une pochette par défaut. Jamais de case vide, et jamais d'attente quand ton PC est éteint : un seul échec
 * suffit pour ne plus le demander pendant 30 s.
 */
class ArtworkProvider : ContentProvider() {

    override fun onCreate() = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        if (mode != "r") throw SecurityException("Lecture seule")
        val seg = uri.pathSegments
        if (seg.size == 1 && seg[0] == "defaut") return open(defaultCover())
        if (seg.size == 2 && seg[0] == "icone") return open(icon(seg[1]) ?: defaultCover())
        val (kind, id) = seg.takeIf { it.size == 2 }?.let { it[0] to it[1].toLongOrNull() }
            ?: throw FileNotFoundException(uri.toString())
        if (id == null || kind !in setOf("release", "track")) throw FileNotFoundException(uri.toString())
        val app = App.instance

        // Gardée avec le morceau ou la sortie : immédiat, même sans réseau
        val local = if (kind == "track") app.downloads.localCover(id) else app.downloads.localReleaseCover(id)
        if (local != null) return open(local)

        val dir = File(context!!.cacheDir, "pochettes").apply { mkdirs() }
        val file = File(dir, "$kind-$id.jpg")
        val now = System.currentTimeMillis()
        // Une semaine en cache : les pochettes changent rarement
        val fresh = file.exists() && now - file.lastModified() < 7L * 24 * 3600 * 1000
        val key = "$kind-$id"
        if (!fresh && !app.serverDown() && now - (missing[key] ?: 0L) > MISSING_TTL) {
            val url = if (kind == "release") app.api.releaseCoverUrl(id, SIZE) else app.api.coverUrl(id, SIZE)
            try {
                val c = URL(url).openConnection() as HttpURLConnection
                c.connectTimeout = 4000
                c.readTimeout = 10000
                try {
                    if (c.responseCode == 200) {
                        val part = File(dir, "$kind-$id.part")
                        c.inputStream.use { input -> part.outputStream().use { input.copyTo(it) } }
                        if (part.length() > 0) part.renameTo(file)
                    } else {
                        // Pas de pochette sur ton PC (la plupart des idées) : on ne redemande pas à chaque ligne
                        missing[key] = now
                    }
                } finally {
                    c.disconnect()
                }
            } catch (e: IOException) {
                // Ton PC ne répond pas : les autres lignes ne l'attendent pas
                app.markServerDown()
                missing[key] = now
            }
        }
        // Même ancienne, une pochette vaut mieux qu'une case vide
        return open(if (file.exists()) file else defaultCover())
    }

    private fun open(f: File) = ParcelFileDescriptor.open(f, ParcelFileDescriptor.MODE_READ_ONLY)

    /** Le rond d'Otoko sur fond nuit, dessiné une fois */
    private fun defaultCover(): File {
        val f = File(context!!.cacheDir, "pochette-defaut.jpg")
        if (f.exists() && f.length() > 0) return f
        synchronized(this) {
            if (f.exists() && f.length() > 0) return f
            val px = SIZE
            val bmp = Bitmap.createBitmap(px, px, Bitmap.Config.ARGB_8888)
            val c = Canvas(bmp)
            c.drawColor(0xFF161924.toInt())
            val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = 0xFFFF6A3D.toInt(); style = Paint.Style.STROKE; strokeWidth = px * 0.075f
            }
            c.drawCircle(px / 2f, px / 2f, px * 0.2f, ring)
            val tmp = File(context!!.cacheDir, "pochette-defaut.part")
            tmp.outputStream().use { bmp.compress(Bitmap.CompressFormat.JPEG, 90, it) }
            bmp.recycle()
            tmp.renameTo(f)
        }
        return f
    }

    /** Une icône de l'appli en image (lecture aléatoire, PC injoignable) : la voiture affiche mal les vecteurs */
    private fun icon(name: String): File? {
        val res = ICONS[name] ?: return null
        val f = File(context!!.cacheDir, "icone-$name.png")
        if (f.exists() && f.length() > 0) return f
        synchronized(this) {
            if (f.exists() && f.length() > 0) return f
            val px = SIZE
            val bmp = Bitmap.createBitmap(px, px, Bitmap.Config.ARGB_8888)
            val c = Canvas(bmp)
            c.drawColor(0xFF1E2230.toInt())
            val d = context!!.getDrawable(res) ?: return null
            val m = (px * 0.28f).toInt()
            d.setBounds(m, m, px - m, px - m)
            d.setTint(0xFFECEEF6.toInt())
            d.draw(c)
            val tmp = File(context!!.cacheDir, "icone-$name.part")
            tmp.outputStream().use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bmp.recycle()
            tmp.renameTo(f)
        }
        return f
    }

    private val missing = ConcurrentHashMap<String, Long>()

    override fun getType(uri: Uri) = if (uri.pathSegments.firstOrNull() == "icone") "image/png" else "image/jpeg"
    override fun query(uri: Uri, p: Array<out String>?, s: String?, a: Array<out String>?, o: String?): Cursor? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, s: String?, a: Array<out String>?) = 0
    override fun update(uri: Uri, v: ContentValues?, s: String?, a: Array<out String>?) = 0

    companion object {
        const val AUTHORITY = "com.otoko.mobile.artwork"
        /** Assez grand pour l'écran de lecture de la voiture, assez léger pour les listes */
        private const val SIZE = 480
        private const val MISSING_TTL = 10 * 60 * 1000L
        fun trackUri(id: Long): Uri = Uri.parse("content://$AUTHORITY/track/$id")
        fun releaseUri(id: Long): Uri = Uri.parse("content://$AUTHORITY/release/$id")
        val defaultUri: Uri = Uri.parse("content://$AUTHORITY/defaut")
        fun iconUri(name: String): Uri = Uri.parse("content://$AUTHORITY/icone/$name")
        private val ICONS = mapOf("aleatoire" to R.drawable.ic_auto_aleatoire, "hors_ligne" to R.drawable.ic_auto_hors_ligne)
    }
}
