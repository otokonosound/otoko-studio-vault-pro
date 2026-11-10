package com.otoko.mobile

import android.content.Context
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

/**
 * Ce que tu changes depuis le téléphone (garder, rejeter, statut, notes, écoutes) part tout de suite vers ton PC.
 * Sans réseau (dans la voiture, en avion), ça attend ici, dans l'ordre, et part dès que ton PC répond.
 */
class Outbox(context: Context, private val api: Api) {
    private val prefs = context.getSharedPreferences("a_envoyer", Context.MODE_PRIVATE)
    private val lock = Mutex()

    private fun pending(): JSONArray = runCatching { JSONArray(prefs.getString("file", "[]")) }.getOrDefault(JSONArray())
    private fun keep(a: JSONArray) { prefs.edit().putString("file", a.toString()).apply() }

    /** Combien de changements attendent ton PC */
    fun waiting(): Int = pending().length()

    /**
     * Envoie maintenant ; sans réseau, garde pour plus tard. Rend la réponse de ton PC, ou null si c'est gardé.
     * Un refus de ton PC (morceau supprimé entre-temps…) remonte tel quel : il ne servirait à rien de réessayer.
     */
    suspend fun send(path: String, body: JSONObject = JSONObject()): JSONObject? = lock.withLock {
        // Ce qui attend part d'abord : l'ordre des changements est gardé.
        if (!flushLocked()) { add(path, body); return null }
        return try {
            api.post(path, body)
        } catch (e: Api.ServerError) {
            throw e
        } catch (e: Exception) {
            add(path, body)
            null
        }
    }

    /** Envoie ce qui attend. Rend vrai si tout est parti (ou s'il n'y avait rien). */
    suspend fun flush(): Boolean = lock.withLock { flushLocked() }

    private suspend fun flushLocked(): Boolean {
        val a = pending()
        var sent = 0
        while (sent < a.length()) {
            val o = a.getJSONObject(sent)
            try {
                api.post(o.getString("path"), o.optJSONObject("body") ?: JSONObject())
            } catch (e: Api.ServerError) {
                // Refusé par ton PC : on l'oublie.
            } catch (e: Exception) {
                break
            }
            sent++
        }
        if (sent > 0) {
            val rest = JSONArray()
            for (i in sent until a.length()) rest.put(a.get(i))
            keep(rest)
        }
        return sent == a.length()
    }

    private fun add(path: String, body: JSONObject) {
        val a = pending()
        // Une liste bornée : un téléphone resté des mois hors réseau ne grossit pas sans fin.
        if (a.length() >= MAX) return
        a.put(JSONObject().put("path", path).put("body", body))
        keep(a)
    }

    companion object {
        private const val MAX = 500
    }
}
