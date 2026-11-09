package com.otoko.mobile

/**
 * Ce qui est gardé sur le téléphone, rangé par sortie : l'appli et Android Auto montrent le même rangement.
 * Les sorties les plus récentes d'abord, chaque sortie dans l'ordre de l'album, puis les morceaux sans sortie.
 */
data class OfflineGroup(
    /** Identifiant stable : « release:12 » ou « none » */
    val key: String,
    val release: ReleaseRef?,
    val tracks: List<Track>,
) {
    val title: String get() = release?.title ?: "Sans sortie"

    /** « EP · 7 morceaux » */
    val subtitle: String
        get() = listOfNotNull(release?.typeLabel?.ifBlank { null }, morceaux(tracks.size)).joinToString(" · ")
}

fun morceaux(n: Int): String = if (n == 1) "1 morceau" else "$n morceaux"

fun offlineGroups(saved: List<Track>): List<OfflineGroup> {
    val byRelease = saved.groupBy { it.release?.id?.takeIf { id -> id > 0 } }
    val releases = byRelease.filterKeys { it != null }.map { (id, tracks) ->
        OfflineGroup("release:$id", tracks.first().release, tracks.sortedWith(albumOrder))
    }.sortedWith(compareByDescending<OfflineGroup> { it.release?.date.orEmpty() }.thenBy { it.title.lowercase() })
    val loose = byRelease[null]?.let { listOf(OfflineGroup("none", null, it.sortedBy { t -> t.title.lowercase() })) }.orEmpty()
    return releases + loose
}

/** L'ordre de l'album quand le PC l'a donné, sinon l'ordre alphabétique */
private val albumOrder = compareBy<Track>({ (it.release?.position ?: 0).let { p -> if (p > 0) p else Int.MAX_VALUE } }, { it.title.lowercase() })

/** Les morceaux, tous groupes à la suite : l'ordre de « Tout écouter » */
fun List<OfflineGroup>.allTracks(): List<Track> = flatMap { it.tracks }
