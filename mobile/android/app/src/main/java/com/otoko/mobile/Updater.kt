package com.otoko.mobile

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.net.Uri
import android.os.Build
import android.provider.Settings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/**
 * La mise à jour depuis le PC, faite par l'appli elle-même : elle télécharge la nouvelle version
 * par le même chemin que la musique, puis la confie à Android. Le navigateur n'y est plus pour rien
 * (VPN qui ne laisse pas passer le Wi-Fi de la maison, téléchargements HTTP bloqués…).
 */
object Updater {
    private const val ACTION = "com.otoko.mobile.MISE_A_JOUR"

    /** Android doit d'abord autoriser l'appli à installer des applications (une seule fois). */
    fun needsPermission(context: Context): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !context.packageManager.canRequestPackageInstalls()

    fun openPermissionSettings(context: Context) {
        val i = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        runCatching { context.startActivity(i) }
    }

    /**
     * Télécharge l'APK et lance l'installation (Android affiche ensuite sa propre confirmation).
     * [progress] reçoit un pourcentage, ou -1 si la taille est inconnue.
     */
    suspend fun install(context: Context, url: String, progress: (Int) -> Unit) = withContext(Dispatchers.IO) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setAppPackageName(context.packageName)
        val id = installer.createSession(params)
        try {
            installer.openSession(id).use { session ->
                val c = URL(url).openConnection() as HttpURLConnection
                c.connectTimeout = 15000
                c.readTimeout = 30000
                try {
                    val code = c.responseCode
                    if (code != 200) throw IOException("Le PC a répondu $code")
                    val size = c.contentLengthLong
                    session.openWrite("otoko.apk", 0, size).use { out ->
                        c.inputStream.use { inp ->
                            val buf = ByteArray(64 * 1024)
                            var done = 0L
                            var last = -2
                            while (true) {
                                val n = inp.read(buf)
                                if (n < 0) break
                                out.write(buf, 0, n)
                                done += n
                                val pct = if (size > 0) (done * 100 / size).toInt() else -1
                                if (pct != last) { last = pct; progress(pct) }
                            }
                            if (size > 0 && done != size) throw IOException("Téléchargement incomplet")
                        }
                        session.fsync(out)
                    }
                } finally {
                    c.disconnect()
                }
                val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                    (if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0)
                val callback = PendingIntent.getBroadcast(
                    context, id, Intent(context, UpdateReceiver::class.java).setAction(ACTION), flags,
                )
                session.commit(callback.intentSender)
            }
        } catch (e: Exception) {
            runCatching { installer.abandonSession(id) }
            throw e
        }
    }
}

/** La réponse d'Android : il demande la confirmation de l'utilisateur, ou dit pourquoi ça n'a pas marché. */
class UpdateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                else intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
                confirm?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)?.let { runCatching { context.startActivity(it) } }
            }
            PackageInstaller.STATUS_SUCCESS -> {}
            else -> {
                val why = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE).orEmpty()
                android.widget.Toast.makeText(context, "Mise à jour non installée. $why".trim(), android.widget.Toast.LENGTH_LONG).show()
            }
        }
    }
}
