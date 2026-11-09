package com.otoko.mobile.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.SubcomposeAsyncImage
import com.otoko.mobile.App
import com.otoko.mobile.DlState
import com.otoko.mobile.Release
import com.otoko.mobile.Track

/** Pochette ; sans image, le disque coloré de la maquette */
@Composable
fun Cover(track: Track?, size: Dp, radius: Dp, modifier: Modifier = Modifier, hiRes: Boolean = false) {
    val shape = RoundedCornerShape(radius)
    Box(modifier.size(size).clip(shape)) {
        val fallback = @Composable { CoverFallback(track) }
        // Gardée hors ligne : elle s'affiche même sans ton PC
        val covers by App.instance.downloads.covers.collectAsState()
        if (track == null) fallback() else SubcomposeAsyncImage(
            model = App.instance.downloads.localCover(track.id).takeIf { track.id in covers }
                ?: App.instance.api.coverUrl(track.id, if (hiRes) 600 else 160),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.fillMaxSize(),
            loading = { fallback() },
            error = { fallback() },
        )
    }
}

@Composable
private fun CoverFallback(track: Track?) {
    val seed = (track?.id ?: 0L).toInt()
    Canvas(Modifier.fillMaxSize().background(accentOf(track))) {
        val d = size.minDimension * (0.55f + (seed % 5) * 0.06f)
        val x = size.width * (0.15f + (seed % 7) * 0.06f)
        val y = size.height * (0.12f + (seed % 3) * 0.1f)
        drawCircle(discOf(track), radius = d / 2, center = Offset(x + d / 2, y + d / 2))
    }
}

/** Pochette d'une sortie ; sans image, un aplat de sa couleur */
@Composable
fun ReleaseCover(release: Release, modifier: Modifier = Modifier, radius: Dp = 8.dp, px: Int = 600) {
    Box(modifier.clip(RoundedCornerShape(radius))) {
        val fallback = @Composable {
            Box(Modifier.fillMaxSize().background(
                Brush.linearGradient(listOf(releaseAccent(release), typeColor(release.type).copy(alpha = 0.6f)))))
        }
        SubcomposeAsyncImage(
            model = App.instance.api.releaseCoverUrl(release.id, px),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.fillMaxSize(),
            loading = { fallback() },
            error = { fallback() },
        )
    }
}

/** Pastille du type de sortie : « EP », « Album »… */
@Composable
fun TypePill(type: String, label: String, modifier: Modifier = Modifier, small: Boolean = false) {
    if (label.isBlank()) return
    Box(
        modifier.height(if (small) 18.dp else 26.dp)
            .background(typeColor(type), RoundedCornerShape(if (small) 9.dp else 13.dp))
            .padding(horizontal = if (small) 6.dp else 10.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Palette.Black, fontFamily = Body, fontWeight = FontWeight.SemiBold,
            fontSize = if (small) 11.sp else 13.sp, maxLines = 1)
    }
}

/** Sous-titre d'un morceau : pastille du type + titre de la sortie, sinon « Original, hypnotique » */
@Composable
fun TrackSubtitle(t: Track, color: Color, fontSize: TextUnit, modifier: Modifier = Modifier, withDate: Boolean = false) {
    val r = t.release
    val date = if (withDate && t.publishedDate.isNotBlank()) " · " + formatDateFr(t.publishedDate) else ""
    if (r == null || r.typeLabel.isBlank()) {
        Text(t.subtitle + date, color = color, fontFamily = Body, fontSize = fontSize, maxLines = 1,
            overflow = TextOverflow.Ellipsis, modifier = modifier)
        return
    }
    Row(modifier, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        TypePill(r.type, r.typeLabel, small = true)
        Text(r.title + date, color = color, fontFamily = Body, fontSize = fontSize, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Trois barres d'égaliseur, figées en pause */
@Composable
fun EqBars(playing: Boolean, modifier: Modifier = Modifier, barHeight: Dp = 20.dp) {
    val inf = rememberInfiniteTransition(label = "eq")
    val periods = listOf(900, 700, 1100)
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.Bottom) {
        periods.forEachIndexed { i, ms ->
            val anim by inf.animateFloat(
                initialValue = 0.25f, targetValue = 1f,
                animationSpec = infiniteRepeatable(tween(ms / 2), RepeatMode.Reverse), label = "bar$i")
            Box(
                Modifier.width(4.dp).height(barHeight)
                    .graphicsLayer {
                        scaleY = if (playing) anim else 0.35f + 0.2f * i % 3
                        transformOrigin = TransformOrigin(0.5f, 1f)
                    }
                    .background(Palette.White, RoundedCornerShape(2.dp)))
        }
    }
}

@Composable
fun CamelotPill(track: Track, modifier: Modifier = Modifier, withKey: Boolean = false) {
    if (track.camelot.isBlank()) return
    Box(
        modifier.height(if (withKey) 30.dp else 24.dp)
            .background(camelotColor(track.camelot), RoundedCornerShape(15.dp))
            .padding(horizontal = if (withKey) 12.dp else 8.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            if (withKey && track.key.isNotBlank()) "${track.camelot} · ${track.key}" else track.camelot,
            color = Palette.Black, fontSize = if (withKey) 14.sp else 13.sp, fontWeight = FontWeight.SemiBold,
            fontFamily = Body)
    }
}

/** Flèche → anneau de progression → coche qui « pop » */
@Composable
fun DownloadButton(state: DlState?, accent: Color, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(CircleShape).pointerInput(state) { detectTapGestures { onClick() } },
        contentAlignment = Alignment.Center,
    ) {
        when (state) {
            null -> Icon(Icons.Download, "Télécharger", tint = Palette.Dim, modifier = Modifier.size(22.dp))
            is DlState.Failed -> Icon(Icons.Retry, "Réessayer", tint = Color(0xFFFF8A80), modifier = Modifier.size(22.dp))
            is DlState.Running -> {
                val p by animateFloatAsState(state.progress, tween(300), label = "dl")
                Canvas(Modifier.size(24.dp)) {
                    val sw = 2.4.dp.toPx()
                    val inset = sw / 2
                    val arcSize = Size(size.width - sw, size.height - sw)
                    drawArc(Palette.Ring, 0f, 360f, false, Offset(inset, inset), arcSize, style = Stroke(sw))
                    drawArc(lerpLight(accent), -90f, 360f * p.coerceAtLeast(0.03f), false, Offset(inset, inset), arcSize,
                        style = Stroke(sw, cap = StrokeCap.Round))
                }
            }
            DlState.Done -> {
                val pop = remember { Animatable(0.3f) }
                LaunchedEffect(Unit) {
                    pop.animateTo(1f, spring(dampingRatio = 0.45f, stiffness = Spring.StiffnessMediumLow))
                }
                Box(
                    Modifier.size(24.dp).scale(pop.value).background(Palette.White, CircleShape),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Check, "Sur le téléphone", tint = Palette.Black, modifier = Modifier.size(14.dp)) }
            }
        }
    }
}

/** L'anneau doit ressortir sur fond noir, même si la pochette est sombre */
private fun lerpLight(c: Color): Color = androidx.compose.ui.graphics.lerp(c, Color.White, 0.45f)

/** Forme d'onde : partie écoutée en blanc, toucher ou glisser pour avancer */
@Composable
fun WaveformSeek(peaks: List<Float>, fraction: Float, onSeek: (Float) -> Unit, modifier: Modifier = Modifier) {
    var dragging by remember { mutableStateOf<Float?>(null) }
    val shown = dragging ?: fraction
    val bars = peaks.ifEmpty { List(64) { 0.35f } }
    BoxWithConstraints(modifier.fillMaxWidth().height(56.dp)) {
        val widthPx = constraints.maxWidth.toFloat()
        Canvas(
            Modifier.fillMaxSize()
                .pointerInput(Unit) { detectTapGestures { onSeek((it.x / widthPx).coerceIn(0f, 1f)) } }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(
                        onDragStart = { dragging = (it.x / widthPx).coerceIn(0f, 1f) },
                        onDragEnd = { dragging?.let(onSeek); dragging = null },
                        onDragCancel = { dragging = null },
                    ) { change, _ -> dragging = (change.position.x / widthPx).coerceIn(0f, 1f) }
                }
        ) {
            val n = bars.size
            val gap = 2.dp.toPx()
            val bw = (size.width - gap * (n - 1)) / n
            val max = bars.maxOrNull()?.takeIf { it > 0f } ?: 1f
            bars.forEachIndexed { i, v ->
                val h = (size.height * (0.12f + 0.88f * (v / max))).coerceAtMost(size.height)
                val x = i * (bw + gap)
                val played = (i + 0.5f) / n <= shown
                drawRoundRect(
                    if (played) Palette.White else Palette.White.copy(alpha = 0.25f),
                    topLeft = Offset(x, (size.height - h) / 2), size = Size(bw, h),
                    cornerRadius = CornerRadius(bw / 2))
            }
        }
    }
}
