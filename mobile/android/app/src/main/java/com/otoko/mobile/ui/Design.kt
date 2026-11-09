@file:OptIn(ExperimentalTextApi::class)

package com.otoko.mobile.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.otoko.mobile.R
import com.otoko.mobile.Release
import com.otoko.mobile.Track
import java.time.LocalDate
import java.time.temporal.ChronoUnit

// ------------------------------------------------------------------ couleurs

object Palette {
    val Black = Color(0xFF000000)
    val White = Color(0xFFFFFFFF)
    val Muted = Color(0xFFA6A6A6)
    val Dim = Color(0xFF8C8C8C)
    val Track = Color(0xFF222222)
    val Surface = Color(0xFF1A1A1A)
    val Ring = Color(0xFF333333)
    /** L'orange d'OTOKO (« ember »), assombri : le fond quand rien ne joue */
    val Default = Color(0xFF8A3A22)
}

/** Pastille Camelot : une teinte par position sur la roue, plus claire pour les majeures (B) */
fun camelotColor(code: String): Color {
    val n = code.dropLast(1).toIntOrNull() ?: return Color(0xFF555555)
    val minor = code.endsWith("A")
    return Color.hsl(((n - 1) * 30f) % 360f, 0.70f, if (minor) 0.62f else 0.76f)
}

/** Couleur de la pochette, assombrie pour que le texte blanc reste lisible */
fun accentOf(t: Track?): Color {
    if (t == null) return Palette.Default
    val base = t.color?.let { Color(it) }
        ?: camelotColor(t.camelot).let { lerp(it, Palette.Black, 0.35f) }
    var c = base
    var i = 0
    while (c.luminance() > 0.16f && i < 8) { c = lerp(c, Palette.Black, 0.18f); i++ }
    return c
}

/** Une couleur par type de sortie (les mêmes que sur le web) */
fun typeColor(type: String): Color = when (type) {
    "single" -> Color(0xFF5BC0EB)
    "ep" -> Color(0xFFF5B942)
    "album" -> Color(0xFF6D6BFF)
    "remixes" -> Color(0xFFD36BFF)
    "compilation" -> Color(0xFF59C17A)
    "mixtape" -> Color(0xFFFF7043)
    else -> Color(0xFF8F8F9A)
}

/** Couleur de la pochette d'une sortie, assombrie pour le fond en dégradé */
fun releaseAccent(r: Release?): Color {
    if (r == null) return Palette.Default
    var c = r.color?.let { Color(it) } ?: lerp(typeColor(r.type), Palette.Black, 0.35f)
    var i = 0
    while (c.luminance() > 0.2f && i < 8) { c = lerp(c, Palette.Black, 0.18f); i++ }
    return c
}

/** Couleur secondaire de la pochette de remplacement (le disque) */
fun discOf(t: Track?): Color = if (t == null) Color(0xFFFF6A3D) else camelotColor(t.camelot)

/** Fond du lecteur plein écran */
fun deepOf(accent: Color): Color = lerp(accent, Palette.Black, 0.45f)

// ------------------------------------------------------------------ police

private fun bricolage(weight: Int, width: Float = 100f, opsz: Float = 14f) = Font(
    R.font.bricolage, FontWeight(weight),
    variationSettings = FontVariation.Settings(
        FontVariation.weight(weight), FontVariation.width(width), FontVariation.Setting("opsz", opsz)))

val Body = FontFamily(bricolage(400), bricolage(500), bricolage(600), bricolage(700))
/** Gros titres : étroite et serrée, comme dans la maquette */
val Display = FontFamily(bricolage(700, 80f, 96f), bricolage(800, 75f, 96f))

// ------------------------------------------------------------------ icônes (tracés de la maquette)

private fun icon(name: String, d: String, fill: Boolean, strokeWidth: Float = 2f): ImageVector =
    ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
        if (fill) {
            addPath(addPathNodes(d), fill = SolidColor(Color.White))
        } else {
            addPath(addPathNodes(d), stroke = SolidColor(Color.White), strokeLineWidth = strokeWidth,
                strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round)
        }
    }.build()

object Icons {
    val Search = icon("search", "M18,11a7,7 0 1,1 -14,0a7,7 0 1,1 14,0M20,20l-3.5,-3.5", false)
    val Download = icon("download", "M12,4v11M7,10l5,5 5,-5M5,20h14", false, 1.8f)
    val Check = icon("check", "M5,12.5l4.5,4.5L19,7.5", false, 3f)
    val Play = icon("play", "M7,4.5v15l13,-7.5z", true)
    val Pause = icon("pause",
        "M7,5h2a1,1 0 0,1 1,1v12a1,1 0 0,1 -1,1h-2a1,1 0 0,1 -1,-1v-12a1,1 0 0,1 1,-1z" +
            "M15,5h2a1,1 0 0,1 1,1v12a1,1 0 0,1 -1,1h-2a1,1 0 0,1 -1,-1v-12a1,1 0 0,1 1,-1z", true)
    val Prev = icon("prev", "M18,5.5v13L8.5,12zM5,5.5h2.4v13H5z", true)
    val Next = icon("next", "M6,5.5v13l9.5,-6.5zM16.6,5.5H19v13h-2.4z", true)
    val ChevronDown = icon("close", "M6,9l6,6 6,-6", false)
    val Library = icon("library",
        "M10,17a3,3 0 1,1 -6,0a3,3 0 1,1 6,0M20,15a3,3 0 1,1 -6,0a3,3 0 1,1 6,0M10,17V5l10,-2v12", false, 1.8f)
    val Offline = icon("offline", "M12,3v11M7.5,9.5l4.5,4.5 4.5,-4.5M4,15v3a2,2 0 0,0 2,2h12a2,2 0 0,0 2,-2v-3", false, 1.8f)
    val Pulse = icon("pulse", "M2,12h3l3,-7 4,14 3,-9 2,2h5", false)
    val Sort = icon("sort", "M4,7h16M7,12h10M10,17h4", false)
    /** Diffuser vers un appareil (enceinte réseau) */
    val Cast = icon("cast", "M3,17a4,4 0 0,1 4,4M3,13a8,8 0 0,1 8,8M3,9a12,12 0 0,1 12,12M3,7V5a2,2 0 0,1 2,-2h14a2,2 0 0,1 2,2v14a2,2 0 0,1 -2,2h-2", false, 1.8f)
    /** Note de musique : « Enregistrer dans Musique » */
    val Music = icon("music", "M9,18V5l11,-2v13M9,18a3,3 0 1,1 -6,0a3,3 0 1,1 6,0zM20,16a3,3 0 1,1 -6,0a3,3 0 1,1 6,0z", false, 1.8f)
    val Trash = icon("trash", "M5,7h14M10,11v6M14,11v6M6,7l1,13h10l1,-13M9,7V4h6v3", false, 1.8f)
    val Retry = icon("retry", "M20,12a8,8 0 1,1 -2.3,-5.6M20,4v5h-5", false)
    val Disc = icon("disc", "M21,12a9,9 0 1,1 -18,0a9,9 0 1,1 18,0M14.5,12a2.5,2.5 0 1,1 -5,0a2.5,2.5 0 1,1 5,0", false, 1.8f)
    val Back = icon("back", "M15,5l-7,7 7,7", false)
    val External = icon("external", "M14,4h6v6M20,4l-9,9M18,14v4a2,2 0 0,1 -2,2H6a2,2 0 0,1 -2,-2V8a2,2 0 0,1 2,-2h4", false, 1.8f)
    /** Partager (trois points reliés, comme sur le web) */
    val Share = icon("share",
        "M21,5a3,3 0 1,1 -6,0a3,3 0 1,1 6,0M9,12a3,3 0 1,1 -6,0a3,3 0 1,1 6,0M21,19a3,3 0 1,1 -6,0a3,3 0 1,1 6,0" +
            "M8.6,13.5l6.8,4M15.4,6.5l-6.8,4", false, 1.8f)
    val Lock = icon("lock", "M6,11h12v9H6zM8.5,11V8a3.5,3.5 0 0,1 7,0v3", false, 1.8f)
    val Copy = icon("copy", "M9,9h11v11H9zM5,15H4V4h11v1", false, 1.8f)
    val Plus = icon("plus", "M12,5v14M5,12h14", false)
    val Shuffle = icon("shuffle", "M16,3h5v5M4,20L21,3M21,16v5h-5M15,15l6,6M4,4l5,5", false, 1.9f)
    val Repeat = icon("repeat", "M17,2l4,4 -4,4M3,11V10a4,4 0 0,1 4,-4h14M7,22l-4,-4 4,-4M21,13v1a4,4 0 0,1 -4,4H3", false, 1.9f)
    val Close = icon("x", "M6,6l12,12M18,6l-12,12", false)
}

// ------------------------------------------------------------------ dates (en français, sans dépendre de la langue du téléphone)

private val MONTHS = listOf("janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août",
    "septembre", "octobre", "novembre", "décembre")

private fun parseDate(s: String): LocalDate? = runCatching { LocalDate.parse(s.take(10)) }.getOrNull()

/** "2026-10-01" → "1 octobre 2026" */
fun formatDateFr(s: String): String = parseDate(s)?.let { "${it.dayOfMonth} ${MONTHS[it.monthValue - 1]} ${it.year}" } ?: s

fun yearOf(s: String): String = parseDate(s)?.year?.toString() ?: ""

/** "Aujourd'hui", "Demain", "22 jours" */
fun countdown(s: String): String {
    val d = parseDate(s) ?: return ""
    val days = ChronoUnit.DAYS.between(LocalDate.now(), d)
    return when {
        days <= 0 -> "Aujourd'hui"
        days == 1L -> "Demain"
        else -> "$days jours"
    }
}

/** "5 titres · 18 min" */
fun releaseStats(tracks: Int, seconds: Double): String {
    val min = (seconds / 60).toInt().let { if (seconds > 0 && it == 0) 1 else it }
    val dur = if (min >= 60) "${min / 60} h ${"%02d".format(min % 60)}" else "$min min"
    val n = if (tracks == 1) "1 titre" else "$tracks titres"
    return if (seconds > 0) "$n · $dur" else n
}

fun formatTime(ms: Long): String {
    val s = (ms / 1000).coerceAtLeast(0)
    return "%d:%02d".format(s / 60, s % 60)
}

fun formatBytes(b: Long): String = when {
    b >= 1_000_000_000 -> "%.1f Go".format(b / 1e9)
    b >= 1_000_000 -> "%.0f Mo".format(b / 1e6)
    else -> "%.0f Ko".format(b / 1e3)
}
