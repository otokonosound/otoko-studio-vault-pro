package com.otoko.mobile.ui

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.otoko.mobile.DlState
import com.otoko.mobile.MainViewModel
import com.otoko.mobile.Release
import com.otoko.mobile.ShareRequest
import com.otoko.mobile.Track
import com.otoko.mobile.UiState
import kotlin.math.roundToInt

// ------------------------------------------------------------------ écran Sorties

@Composable
fun ReleasesScreen(ui: UiState, vm: MainViewModel, accent: Color, onMessage: (String) -> Unit) {
    val states by vm.downloads.states.collectAsState()
    val upcoming = ui.releases.filter { it.upcoming }.sortedBy { it.date }
    // La dernière sortie parue ; à défaut, la prochaine
    val hero = ui.releases.firstOrNull { !it.upcoming } ?: upcoming.firstOrNull()
    val soon = upcoming.filter { it.id != hero?.id }

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 110.dp)) {
        item(key = "hero") {
            if (hero != null) ReleaseHero(hero, vm, onMessage)
            else Column(
                Modifier.fillMaxWidth().background(Brush.verticalGradient(listOf(Palette.Default, Palette.Black)))
                    .statusBarsPadding().padding(start = 20.dp, end = 20.dp, top = 14.dp, bottom = 28.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                PageTitle()
                Text(
                    when {
                        ui.releasesError != null -> ui.releasesError
                        ui.releasesLoading -> "Chargement…"
                        else -> "Aucune sortie pour l'instant."
                    },
                    fontFamily = Body, fontSize = 16.sp, color = Palette.White.copy(alpha = 0.85f))
            }
        }
        if (hero != null && ui.releasesError != null) item(key = "err") {
            Text(ui.releasesError, color = Palette.Muted, fontFamily = Body, fontSize = 14.sp,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
        }

        if (soon.isNotEmpty()) {
            item(key = "t-soon") { SectionTitle("À venir") }
            items(soon, key = { "soon-${it.id}" }) { r -> UpcomingRow(r) { vm.openRelease(r.id) } }
        }

        if (ui.latestPublished.isNotEmpty()) {
            item(key = "t-pub") { SectionTitle("Derniers morceaux publiés") }
            itemsIndexed(ui.latestPublished, key = { _, t -> "pub-${t.id}" }) { i, t ->
                Box(Modifier.padding(start = 20.dp, end = 8.dp)) {
                    TrackRow(
                        t, i, 0,
                        isCurrent = ui.current?.id == t.id, playing = ui.playing, accent = accent,
                        dl = states[t.id],
                        onPlay = { vm.play(ui.latestPublished, i) },
                        onDownload = { downloadOrExplain(t, states[t.id], vm, onMessage) },
                        withDate = true,
                    )
                }
            }
        }

        if (ui.releases.isNotEmpty()) {
            item(key = "t-all") { SectionTitle("Toutes les sorties") }
            items(ui.releases.chunked(2), key = { "grid-${it.first().id}" }) { pair ->
                Row(
                    Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp),
                    horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    pair.forEach { r ->
                        ReleaseTile(r, Modifier.weight(1f), onLongPress = { vm.downloadRelease(r.id, onMessage = onMessage) }) {
                            vm.openRelease(r.id)
                        }
                    }
                    if (pair.size == 1) Spacer(Modifier.weight(1f))
                }
            }
        }
    }
}

@Composable
private fun PageTitle() {
    Text("Sorties", fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 44.sp, lineHeight = 42.sp,
        letterSpacing = (-1.5).sp, color = Palette.White)
}

@Composable
private fun SectionTitle(text: String) {
    Text(text, fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 26.sp, letterSpacing = (-0.6).sp,
        color = Palette.White, modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 26.dp, bottom = 10.dp))
}

@Composable
private fun ReleaseHero(r: Release, vm: MainViewModel, onMessage: (String) -> Unit) {
    Column(
        Modifier.fillMaxWidth().background(Brush.verticalGradient(listOf(releaseAccent(r), Palette.Black)))
            .statusBarsPadding().padding(start = 20.dp, end = 20.dp, top = 14.dp, bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        PageTitle()
        Text(if (r.upcoming) "Prochaine sortie" else "Dernière sortie", fontFamily = Body, fontWeight = FontWeight.SemiBold,
            fontSize = 14.sp, letterSpacing = 0.5.sp, color = Palette.White.copy(alpha = 0.85f))
        ReleaseCover(
            r,
            Modifier.fillMaxWidth().aspectRatio(1f).shadow(24.dp, RoundedCornerShape(10.dp))
                .clickable { vm.openRelease(r.id) },
            radius = 10.dp)
        ReleaseInfo(r, descLines = 3, onPlay = { vm.playRelease(r.id, onMessage) })
    }
}

/** Titre, type, date, « 5 titres · 18 min », description, boutons */
@Composable
private fun ReleaseInfo(r: Release, descLines: Int, onPlay: () -> Unit, extra: (@Composable () -> Unit)? = null) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(r.title, fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 34.sp, lineHeight = 36.sp,
            letterSpacing = (-1).sp, color = Palette.White, maxLines = 3, overflow = TextOverflow.Ellipsis)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            TypePill(r.type, r.typeLabel)
            Text(if (r.upcoming) "Sort le ${formatDateFr(r.date)}" else formatDateFr(r.date),
                fontFamily = Body, fontSize = 15.sp, color = Palette.White.copy(alpha = 0.9f))
        }
        Text(releaseStats(r.tracks, r.duration), fontFamily = Body, fontSize = 14.sp, color = Palette.White.copy(alpha = 0.7f))
        if (r.description.isNotBlank()) Text(
            r.description, fontFamily = Body, fontSize = 15.sp, lineHeight = 21.sp, color = Palette.White.copy(alpha = 0.85f),
            maxLines = descLines, overflow = TextOverflow.Ellipsis)
        Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(
                Modifier.height(48.dp).clip(RoundedCornerShape(24.dp)).background(Palette.White)
                    .clickable(onClick = onPlay).padding(horizontal = 20.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(Icons.Play, null, tint = Palette.Black, modifier = Modifier.size(18.dp))
                Text("Tout écouter", color = Palette.Black, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
            }
            extra?.invoke()
            if (r.soundcloud.isNotBlank()) {
                val context = LocalContext.current
                Row(
                    Modifier.height(48.dp).clip(RoundedCornerShape(24.dp)).background(Color.White.copy(alpha = 0.14f))
                        .clickable {
                            runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(r.soundcloud))) }
                        }
                        .padding(horizontal = 18.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Icon(Icons.External, null, tint = Palette.White, modifier = Modifier.size(18.dp))
                    Text("SoundCloud", color = Palette.White, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
                }
            }
        }
    }
}

@Composable
private fun UpcomingRow(r: Release, onOpen: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(76.dp).clickable(onClick = onOpen).padding(horizontal = 20.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        ReleaseCover(r, Modifier.size(56.dp), radius = 6.dp, px = 160)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(r.title, color = Palette.White, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 17.sp,
                maxLines = 1, overflow = TextOverflow.Ellipsis)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                TypePill(r.type, r.typeLabel, small = true)
                Text(formatDateFr(r.date), color = Palette.Muted, fontFamily = Body, fontSize = 14.sp, maxLines = 1)
            }
        }
        Text(countdown(r.date), color = Palette.White, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
    }
}

@Composable
@OptIn(ExperimentalFoundationApi::class)
private fun ReleaseTile(r: Release, modifier: Modifier, onLongPress: () -> Unit, onOpen: () -> Unit) {
    // Appui long : toute la sortie hors ligne
    Column(modifier.clip(RoundedCornerShape(10.dp)).combinedClickable(onLongClick = onLongPress, onClick = onOpen)) {
        ReleaseCover(r, Modifier.fillMaxWidth().aspectRatio(1f), radius = 8.dp, px = 400)
        Text(r.title, color = Palette.White, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp,
            maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 8.dp))
        if (r.upcoming) {
            Text("Sort le ${formatDateFr(r.date)}", color = Palette.Muted, fontFamily = Body, fontSize = 13.sp,
                modifier = Modifier.padding(top = 2.dp))
            Box(
                Modifier.padding(top = 6.dp).height(20.dp).background(Palette.White, RoundedCornerShape(10.dp))
                    .padding(horizontal = 8.dp),
                contentAlignment = Alignment.Center,
            ) { Text("Bientôt", color = Palette.Black, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 11.sp) }
        } else {
            Text(listOf(r.typeLabel, yearOf(r.date)).filter { it.isNotBlank() }.joinToString(" · "),
                color = Palette.Muted, fontFamily = Body, fontSize = 13.sp, modifier = Modifier.padding(top = 2.dp))
        }
    }
}

/** « Tout télécharger » → « 3 / 5 » pendant le téléchargement → « Téléchargée » (touche : tout retirer) */
@Composable
private fun ReleaseDownloadButton(r: Release, tracks: List<Track>, vm: MainViewModel, onMessage: (String) -> Unit) {
    val states by vm.downloads.states.collectAsState()
    val batches by vm.downloads.batches.collectAsState()
    var confirm by remember { mutableStateOf(false) }
    val batch = batches[r.id]
    val allDone = tracks.all { states[it.id] == DlState.Done }
    val label = when {
        batch != null -> "${batch.first} / ${batch.second}"
        allDone -> "Téléchargée"
        else -> "Tout télécharger"
    }
    Row(
        Modifier.height(48.dp).clip(RoundedCornerShape(24.dp)).background(Color.White.copy(alpha = 0.14f))
            .clickable {
                when {
                    batch != null -> onMessage("Téléchargement en cours : ${batch.first} sur ${batch.second}.")
                    allDone -> confirm = true
                    else -> vm.downloadRelease(r.id, tracks, onMessage)
                }
            }
            .padding(horizontal = 18.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(if (allDone && batch == null) Icons.Check else Icons.Download, null, tint = Palette.White, modifier = Modifier.size(18.dp))
        Text(label, color = Palette.White, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
    }
    if (confirm) AlertDialog(
        onDismissRequest = { confirm = false },
        title = { Text("Retirer la sortie du téléphone ?") },
        text = { Text("Les ${tracks.size} titres de « ${r.title} » seront retirés de l'appli. Les copies dans Musique restent.") },
        confirmButton = {
            TextButton(onClick = { confirm = false; vm.downloads.removeAll(tracks); onMessage("Sortie retirée du téléphone.") }) {
                Text("Tout retirer")
            }
        },
        dismissButton = { TextButton(onClick = { confirm = false }) { Text("Annuler") } },
    )
}

// ------------------------------------------------------------------ fiche d'une sortie

@Composable
fun ReleaseDetail(ui: UiState, vm: MainViewModel, accent: Color, onMessage: (String) -> Unit) {
    val current = ui.releaseView
    BackHandler(current != null) { vm.closeRelease() }
    AnimatedVisibility(
        current != null,
        enter = slideInHorizontally(tween(420, easing = Ease)) { it } + fadeIn(tween(250)),
        exit = slideOutHorizontally(tween(320, easing = Ease)) { it } + fadeOut(tween(250)),
    ) {
        // garde le contenu pendant l'animation de sortie
        val last = remember { mutableStateOf(current) }
        if (current != null) last.value = current
        val view = last.value ?: return@AnimatedVisibility
        val r = view.release
        val states by vm.downloads.states.collectAsState()

        Box(
            Modifier.fillMaxSize().background(Palette.Black)
                .clickable(remember { MutableInteractionSource() }, null) {}
        ) {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 110.dp)) {
                item(key = "head") {
                    Column(
                        Modifier.fillMaxWidth().background(Brush.verticalGradient(listOf(releaseAccent(r), Palette.Black)))
                            .statusBarsPadding().padding(start = 20.dp, end = 20.dp, top = 12.dp, bottom = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(16.dp),
                    ) {
                        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                            Box(
                                Modifier.size(44.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.12f))
                                    .clickable { vm.closeRelease() },
                                contentAlignment = Alignment.Center,
                            ) { Icon(Icons.Back, "Retour", tint = Palette.White, modifier = Modifier.size(22.dp)) }
                            Spacer(Modifier.weight(1f))
                            // Lien d'écoute privée pour cette sortie
                            if (r != null) Row(
                                Modifier.height(44.dp).clip(RoundedCornerShape(22.dp)).background(Color.White.copy(alpha = 0.12f))
                                    .clickable {
                                        vm.openShare(ShareRequest("releases", releases = listOf(r.id to r.title), label = r.title))
                                    }
                                    .padding(horizontal = 16.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Icon(Icons.Share, null, tint = Palette.White, modifier = Modifier.size(18.dp))
                                Text("Partager", color = Palette.White, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
                            }
                        }
                        if (r != null) {
                            ReleaseCover(
                                r,
                                Modifier.align(Alignment.CenterHorizontally).fillMaxWidth(0.86f).aspectRatio(1f)
                                    .shadow(28.dp, RoundedCornerShape(10.dp)),
                                radius = 10.dp)
                            ReleaseInfo(r, descLines = Int.MAX_VALUE, onPlay = { vm.playRelease(r.id, onMessage) },
                                extra = if (view.tracks.isEmpty()) null else {
                                    { ReleaseDownloadButton(r, view.tracks, vm, onMessage) }
                                })
                        }
                    }
                }
                val msg = when {
                    view.error != null -> view.error
                    view.loading -> "Chargement…"
                    view.tracks.isEmpty() -> "Aucun morceau dans cette sortie pour l'instant."
                    else -> null
                }
                if (msg != null) item(key = "msg") {
                    Text(msg, color = Palette.Muted, fontFamily = Body, fontSize = 15.sp,
                        modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp))
                }
                if (view.tracks.isNotEmpty()) item(key = "t-titles") {
                    Text("Titres", fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 26.sp, letterSpacing = (-0.6).sp,
                        color = Palette.White, modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 14.dp, bottom = 6.dp))
                }
                itemsIndexed(view.tracks, key = { _, t -> "rt-${t.id}" }) { i, t ->
                    ReleaseTrackRow(
                        i + 1, t, isCurrent = ui.current?.id == t.id, playing = ui.playing, accent = accent,
                        dl = states[t.id],
                        onPlay = { vm.play(view.tracks, i) },
                        onDownload = { downloadOrExplain(t, states[t.id], vm, onMessage) },
                    )
                }
            }
        }
    }
}

@Composable
private fun ReleaseTrackRow(
    number: Int, t: Track, isCurrent: Boolean, playing: Boolean, accent: Color,
    dl: DlState?, onPlay: () -> Unit, onDownload: () -> Unit,
) {
    val titleColor by animateColorAsState(
        if (isCurrent) lerp(accent, Color.White, 0.55f) else Palette.White, tween(400), label = "rtitle")
    Row(
        Modifier.fillMaxWidth().height(62.dp).padding(start = 20.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier.weight(1f).height(62.dp).clickable(onClick = onPlay),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Box(Modifier.width(26.dp), contentAlignment = Alignment.Center) {
                if (isCurrent) EqBars(playing, barHeight = 14.dp)
                else Text("$number", color = Palette.Dim, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
            }
            Column(Modifier.weight(1f)) {
                Text(t.title, color = titleColor, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 17.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOfNotNull(
                        t.bpm?.let { "${it.roundToInt()} BPM" },
                        t.duration.takeIf { it > 0 }?.let { formatTime((it * 1000).toLong()) },
                    ).joinToString(" · "),
                    color = Palette.Muted, fontFamily = Body, fontSize = 14.sp, maxLines = 1)
            }
            CamelotPill(t, Modifier.width(40.dp))
        }
        DownloadButton(dl, accent, onDownload)
    }
}
