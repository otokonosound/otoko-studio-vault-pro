package com.otoko.mobile.ui

import android.os.StatFs
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.otoko.mobile.DlState
import com.otoko.mobile.Filter
import com.otoko.mobile.MainViewModel
import com.otoko.mobile.Quality
import com.otoko.mobile.Screen
import com.otoko.mobile.ShareRequest
import com.otoko.mobile.Sort
import com.otoko.mobile.Track
import com.otoko.mobile.UiState
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

internal val Ease = CubicBezierEasing(0.2f, 0.85f, 0.2f, 1f)
private const val COLOR_MS = 700

@Composable
fun OtokoApp(vm: MainViewModel) {
    val server by vm.settings.server.collectAsState()
    val suggested by vm.suggested.collectAsState()
    if (server.isBlank() || suggested != null) {
        MaterialTheme(colorScheme = darkColorScheme(surface = Palette.Surface, background = Palette.Black)) {
            ConnectScreen(vm, suggested.orEmpty(), canCancel = server.isNotBlank())
        }
        return
    }
    val ui by vm.ui.collectAsState()
    val accent by animateColorAsState(accentOf(ui.current), tween(COLOR_MS), label = "accent")
    var toast by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(toast) { if (toast != null) { delay(3500); toast = null } }
    var castOpen by remember { mutableStateOf(false) }
    // Messages de la diffusion (Mu-so perdue, morceau indisponible…)
    LaunchedEffect(Unit) { vm.cast.messages.collect { toast = it } }
    // Une nouvelle version proposée par ton PC : un clic la télécharge, Android demande de l'installer
    val context = LocalContext.current
    var updateUrl by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) { updateUrl = runCatching { vm.updateUrl() }.getOrNull() }

    MaterialTheme(colorScheme = darkColorScheme(surface = Palette.Surface, background = Palette.Black)) {
        Box(Modifier.fillMaxSize().background(Palette.Black)) {
            Column(Modifier.fillMaxSize()) {
                Box(Modifier.weight(1f)) {
                    Column(Modifier.fillMaxSize()) {
                        // Les écrans Sorties et Partages ont leur propre en-tête
                        if (ui.screen != Screen.RELEASES && ui.screen != Screen.SHARES) Header(ui, vm, accent)
                        AnimatedContent(
                            ui.screen, label = "screen", modifier = Modifier.weight(1f),
                            transitionSpec = {
                                (fadeIn(tween(380)) + slideInVertically(tween(380, easing = Ease)) { it / 40 })
                                    .togetherWith(fadeOut(tween(150)))
                            },
                        ) { screen ->
                            when (screen) {
                                Screen.RELEASES -> ReleasesScreen(ui, vm, accent) { toast = it }
                                Screen.LIBRARY -> LibraryList(ui, vm, accent) { toast = it }
                                Screen.OFFLINE -> OfflineScreen(ui, vm, accent) { toast = it }
                                Screen.SHARES -> SharesScreen(ui, vm, accent) { toast = it }
                            }
                        }
                    }
                    ReleaseDetail(ui, vm, accent) { toast = it }
                }
                BottomNav(ui.screen) { vm.setScreen(it) }
            }

            AnimatedVisibility(
                ui.current != null,
                enter = slideInVertically(tween(450, easing = Ease)) { it } + fadeIn(),
                modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 72.dp),
            ) { MiniPlayer(ui, vm, accent) { castOpen = true } }

            PlayerSheet(ui, vm, accent) { castOpen = true }
            CastSheet(castOpen, vm, onClose = { castOpen = false }) { toast = it }
            ShareSheet(ui, vm) { toast = it }

            updateUrl?.let { url ->
                Text(
                    "Nouvelle version d'Otoko Vault : installer", color = Palette.Black, fontFamily = Body, fontSize = 14.sp,
                    modifier = Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 8.dp)
                        .background(Palette.White, RoundedCornerShape(14.dp))
                        .clickable {
                            context.startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(url)))
                            updateUrl = null
                        }
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                )
            }

            // Par-dessus tout, pour rester visible au-dessus des feuilles
            AnimatedVisibility(
                toast != null, enter = fadeIn(), exit = fadeOut(),
                modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 156.dp, start = 16.dp, end = 16.dp),
            ) {
                Text(toast ?: "", color = Palette.Black, fontFamily = Body, fontSize = 14.sp,
                    modifier = Modifier.background(Palette.White, RoundedCornerShape(14.dp)).padding(14.dp))
            }
        }
    }
}

// ------------------------------------------------------------------ en-tête

@Composable
private fun Header(ui: UiState, vm: MainViewModel, accent: Color) {
    val isLib = ui.screen == Screen.LIBRARY
    Column(
        Modifier.fillMaxWidth().background(accent).statusBarsPadding()
            .padding(start = 20.dp, end = 20.dp, top = 14.dp, bottom = 18.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
            Text(
                if (isLib) "Bibliothèque" else "Hors ligne",
                fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 44.sp, lineHeight = 42.sp,
                letterSpacing = (-1.5).sp, color = Palette.White, modifier = Modifier.weight(1f))
            val saved by vm.downloads.saved.collectAsState()
            Text(
                if (isLib) (if (ui.loading && ui.tracks.isEmpty()) "…" else "${ui.tracks.size} morceaux")
                else "${saved.size} morceaux",
                fontFamily = Body, fontSize = 14.sp, color = Palette.White.copy(alpha = 0.85f),
                modifier = Modifier.padding(bottom = 4.dp))
        }
        if (isLib) {
            SearchField(ui.query, vm::setQuery)
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                SortChip(ui.sort, vm::setSort)
                Filter.entries.filter { it != Filter.ALL }.forEach { f ->
                    val on = ui.filter == f
                    val bg by animateColorAsState(if (on) Palette.White else Color.Black.copy(alpha = 0.28f), tween(250), label = "chip")
                    val fg by animateColorAsState(if (on) Palette.Black else Palette.White, tween(250), label = "chipfg")
                    Box(
                        Modifier.height(34.dp).clip(RoundedCornerShape(17.dp)).background(bg)
                            .clickable { vm.setFilter(f) }.padding(horizontal = 14.dp),
                        contentAlignment = Alignment.Center,
                    ) { Text(f.label, color = fg, fontFamily = Body, fontWeight = FontWeight.Medium, fontSize = 14.sp) }
                }
            }
        }
    }
}

@Composable
private fun SearchField(value: String, onChange: (String) -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(46.dp).background(Color.Black.copy(alpha = 0.28f), RoundedCornerShape(14.dp))
            .padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Icon(Icons.Search, null, tint = Palette.White.copy(alpha = 0.85f), modifier = Modifier.size(18.dp))
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (value.isEmpty()) Text("Titre, tonalité, BPM, ambiance", color = Palette.White.copy(alpha = 0.7f),
                fontFamily = Body, fontSize = 16.sp, maxLines = 1)
            BasicTextField(
                value, onChange, singleLine = true, cursorBrush = SolidColor(Palette.White),
                textStyle = TextStyle(color = Palette.White, fontFamily = Body, fontSize = 16.sp),
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Rechercher" })
        }
    }
}

@Composable
private fun SortChip(sort: Sort, onPick: (Sort) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier.height(34.dp).clip(RoundedCornerShape(17.dp)).background(Color.Black.copy(alpha = 0.28f))
                .clickable { open = true }.padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(Icons.Sort, null, tint = Palette.White, modifier = Modifier.size(16.dp))
            Text(sort.label, color = Palette.White, fontFamily = Body, fontWeight = FontWeight.Medium, fontSize = 14.sp)
        }
        DropdownMenu(open, { open = false }, modifier = Modifier.background(Palette.Surface)) {
            Sort.entries.forEach { s ->
                DropdownMenuItem(
                    text = {
                        Text(s.label, fontFamily = Body, fontSize = 16.sp,
                            fontWeight = if (s == sort) FontWeight.Bold else FontWeight.Normal,
                            color = if (s == sort) Palette.White else Palette.Muted)
                    },
                    onClick = { open = false; onPick(s) })
            }
        }
    }
}

// ------------------------------------------------------------------ bibliothèque

@Composable
private fun LibraryList(ui: UiState, vm: MainViewModel, accent: Color, onMessage: (String) -> Unit) {
    val states by vm.downloads.states.collectAsState()
    val listState = rememberLazyListState()
    LaunchedEffect(ui.listVersion) { listState.scrollToItem(0) }
    LazyColumn(
        state = listState,
        contentPadding = PaddingValues(start = 20.dp, end = 8.dp, top = 6.dp, bottom = 96.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        if (ui.error != null) item(key = "err") {
            Text(ui.error, color = Palette.Muted, fontFamily = Body, fontSize = 14.sp, modifier = Modifier.padding(vertical = 10.dp))
        }
        if (!ui.loading && ui.tracks.isEmpty() && ui.error == null) item(key = "vide") {
            Text("Aucun morceau ne correspond.", color = Palette.Muted, fontFamily = Body, fontSize = 16.sp,
                modifier = Modifier.padding(vertical = 24.dp))
        }
        itemsIndexed(ui.tracks, key = { _, t -> t.id }) { i, t ->
            TrackRow(
                t, i, ui.listVersion,
                isCurrent = ui.current?.id == t.id, playing = ui.playing, accent = accent,
                dl = states[t.id],
                onPlay = { vm.play(ui.tracks, i) },
                onDownload = { downloadOrExplain(t, states[t.id], vm, onMessage) },
            )
        }
    }
}

/** Flèche de téléchargement d'une ligne de morceau */
internal fun downloadOrExplain(t: Track, state: DlState?, vm: MainViewModel, onMessage: (String) -> Unit) {
    when (state) {
        DlState.Done -> onMessage("Déjà sur le téléphone. Pour le retirer : onglet Hors ligne.")
        is DlState.Running -> {}
        else -> vm.download(t)?.let(onMessage)
    }
}

@Composable
internal fun TrackRow(
    t: Track, index: Int, version: Int, isCurrent: Boolean, playing: Boolean, accent: Color,
    dl: DlState?, onPlay: () -> Unit, onDownload: () -> Unit, withDate: Boolean = false,
) {
    // Entrée en cascade (seulement pour le premier écran de la liste)
    val appear = remember(version) { Animatable(if (index < 14) 0f else 1f) }
    LaunchedEffect(version) {
        if (index < 14) { delay(index * 40L); appear.animateTo(1f, tween(520, easing = Ease)) }
    }
    val density = LocalDensity.current
    val titleColor by animateColorAsState(
        if (isCurrent) androidx.compose.ui.graphics.lerp(accent, Color.White, 0.55f) else Palette.White, tween(400), label = "title")
    Row(
        Modifier.fillMaxWidth().height(66.dp)
            .graphicsLayer { alpha = appear.value; translationY = (1f - appear.value) * with(density) { 14.dp.toPx() } },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier.weight(1f).height(66.dp).clickable(onClick = onPlay),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Box(contentAlignment = Alignment.BottomCenter) {
                Cover(t, 48.dp, 6.dp)
                if (isCurrent) Box(
                    Modifier.size(48.dp).clip(RoundedCornerShape(6.dp)).background(Color.Black.copy(alpha = 0.45f)),
                    contentAlignment = Alignment.BottomCenter,
                ) { EqBars(playing, Modifier.padding(bottom = 12.dp)) }
            }
            Column(Modifier.weight(1f)) {
                Text(t.title, color = titleColor, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 17.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
                TrackSubtitle(t, Palette.Muted, 14.sp, Modifier.padding(top = 2.dp), withDate = withDate)
            }
            if (t.bpm != null) Column(horizontalAlignment = Alignment.End) {
                Text("${t.bpm.roundToInt()}", color = Palette.White, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 17.sp)
                Text("BPM", color = Palette.Dim, fontFamily = Body, fontSize = 12.sp)
            }
            CamelotPill(t, Modifier.width(40.dp))
        }
        DownloadButton(dl, accent, onDownload)
    }
}

// ------------------------------------------------------------------ hors ligne

@Composable
private fun OfflineScreen(ui: UiState, vm: MainViewModel, accent: Color, onMessage: (String) -> Unit) {
    val saved by vm.downloads.saved.collectAsState()
    val states by vm.downloads.states.collectAsState()
    val inMusic by vm.downloads.inMusic.collectAsState()
    val quality by vm.settings.mobileQuality.collectAsState()
    val wifiOnly by vm.settings.wifiOnly.collectAsState()
    val saveToMusic by vm.settings.saveToMusic.collectAsState()
    val connection by vm.connection.collectAsState()
    val context = LocalContext.current
    val used = remember(saved) { vm.downloads.usedBytes() }
    val free = remember(saved) { StatFs(context.filesDir.path).availableBytes }
    val running = states.count { it.value is DlState.Running }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(start = 20.dp, end = 20.dp, top = 22.dp, bottom = 110.dp),
        verticalArrangement = Arrangement.spacedBy(22.dp),
    ) {
        Column {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(formatBytes(used), fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 52.sp,
                    letterSpacing = (-1.5).sp, color = Palette.White, lineHeight = 52.sp)
                Text("sur ce téléphone", fontFamily = Body, fontSize = 16.sp, color = Palette.Muted,
                    modifier = Modifier.padding(bottom = 8.dp))
            }
            val share = if (used + free > 0) used.toFloat() / (used + free) else 0f
            val w by animateFloatAsState(share.coerceAtLeast(0.01f), tween(900, easing = Ease), label = "store")
            Box(Modifier.padding(top = 12.dp).fillMaxWidth().height(10.dp).clip(RoundedCornerShape(5.dp)).background(Palette.Track)) {
                Box(Modifier.fillMaxWidth(w).height(10.dp).background(androidx.compose.ui.graphics.lerp(accent, Color.White, 0.4f)))
            }
            Text(
                "${saved.size} morceaux sans perte · ${formatBytes(free)} libres" +
                    if (running > 0) " · $running en cours" else "",
                fontFamily = Body, fontSize = 14.sp, color = Palette.Muted, modifier = Modifier.padding(top = 8.dp))
        }

        Column {
            Text("Qualité en données mobiles", fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp,
                color = Palette.White, modifier = Modifier.padding(bottom = 10.dp))
            QualitySwitch(quality, vm.settings::setMobileQuality)
            Text(quality.hint + " En Wi-Fi, toujours la qualité d'origine.", fontFamily = Body, fontSize = 14.sp,
                color = Palette.Muted, modifier = Modifier.padding(top = 8.dp))
        }

        Row(
            Modifier.fillMaxWidth().clickable { vm.settings.setWifiOnly(!wifiOnly) }.padding(vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Télécharger seulement en Wi-Fi", fontFamily = Body, fontSize = 16.sp, color = Palette.White,
                modifier = Modifier.weight(1f))
            Toggle(wifiOnly, accent)
        }

        Column {
            Row(
                Modifier.fillMaxWidth().clickable { vm.settings.setSaveToMusic(!saveToMusic) }.padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Enregistrer aussi dans Musique", fontFamily = Body, fontSize = 16.sp, color = Palette.White,
                    modifier = Modifier.weight(1f))
                Toggle(saveToMusic, accent)
            }
            Text(
                "Chaque téléchargement est copié dans Musique › OTOKO, avec titre, artiste, sortie et pochette : " +
                    "les autres applis de musique le voient. Retirer un morceau ici ne supprime pas cette copie.",
                fontFamily = Body, fontSize = 14.sp, color = Palette.Muted, modifier = Modifier.padding(top = 4.dp))
            if (saved.any { it.id !in inMusic }) Row(
                Modifier.padding(top = 12.dp).height(44.dp).clip(RoundedCornerShape(22.dp)).background(Palette.Surface)
                    .clickable { onMessage("Copie dans Musique…"); vm.saveAllToMusic(onMessage) }.padding(horizontal = 18.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(Icons.Music, null, tint = Palette.White, modifier = Modifier.size(18.dp))
                Text("Tout enregistrer dans Musique", color = Palette.White, fontFamily = Body,
                    fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            var changing by remember { mutableStateOf(false) }
            val address by vm.settings.server.collectAsState()
            Text("Connexion : ${connection.label}", fontFamily = Body, fontSize = 14.sp, color = Palette.Muted)
            Text(if (changing) "Annuler" else "Changer l'adresse de mon PC", fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp,
                color = Palette.White, modifier = Modifier.clickable { changing = !changing }.padding(vertical = 4.dp))
            if (changing) ConnectForm(vm, address) { changing = false; onMessage("Appli reliée à ce PC.") }
        }

        Column {
            Text("Sur ce téléphone", fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, color = Palette.White,
                modifier = Modifier.padding(bottom = 4.dp))
            if (saved.isEmpty()) Text(
                "Rien pour l'instant. Touche la flèche à droite d'un morceau dans la bibliothèque pour le garder ici.",
                fontFamily = Body, fontSize = 14.sp, color = Palette.Muted, modifier = Modifier.padding(top = 6.dp))
            saved.forEachIndexed { i, t ->
                Row(
                    Modifier.fillMaxWidth().height(62.dp).clickable { vm.play(saved, i) },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Cover(t, 46.dp, 6.dp)
                    Column(Modifier.weight(1f)) {
                        Text(t.title, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, color = Palette.White,
                            maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(t.qualityLabel + if (t.id in inMusic) " · dans Musique" else "",
                            fontFamily = Body, fontSize = 14.sp, color = Palette.Muted)
                    }
                    Box(
                        Modifier.size(44.dp).clip(CircleShape).clickable {
                            if (t.id in inMusic) onMessage("Déjà dans Musique › OTOKO.") else vm.saveToMusic(t, onMessage)
                        },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(Icons.Music, "Enregistrer dans Musique",
                            tint = if (t.id in inMusic) Palette.White else Palette.Dim, modifier = Modifier.size(20.dp))
                    }
                    Box(
                        Modifier.size(44.dp).clip(CircleShape).clickable { vm.downloads.remove(t.id) },
                        contentAlignment = Alignment.Center,
                    ) { Icon(Icons.Trash, "Retirer du téléphone", tint = Palette.Dim, modifier = Modifier.size(20.dp)) }
                }
            }
        }
    }
}

@Composable
private fun QualitySwitch(value: Quality, onPick: (Quality) -> Unit) {
    BoxWithConstraints(Modifier.fillMaxWidth().height(50.dp).background(Palette.Surface, RoundedCornerShape(14.dp)).padding(4.dp)) {
        val cell = maxWidth / Quality.entries.size
        val x by animateDpAsState(cell * value.ordinal, tween(380, easing = Ease), label = "q")
        Box(Modifier.offset(x = x).width(cell).fillMaxSize().background(Palette.White, RoundedCornerShape(10.dp)))
        Row(Modifier.fillMaxSize()) {
            Quality.entries.forEach { q ->
                val fg by animateColorAsState(if (q == value) Palette.Black else Palette.White, tween(300), label = "qfg")
                Box(
                    Modifier.weight(1f).fillMaxSize().clip(RoundedCornerShape(10.dp))
                        .clickable(remember { MutableInteractionSource() }, null) { onPick(q) },
                    contentAlignment = Alignment.Center,
                ) { Text(q.label, color = fg, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp) }
            }
        }
    }
}

@Composable
private fun Toggle(on: Boolean, accent: Color) {
    val track by animateColorAsState(
        if (on) androidx.compose.ui.graphics.lerp(accent, Color.White, 0.25f) else Palette.Ring, tween(250), label = "tg")
    val x by animateDpAsState(if (on) 20.dp else 0.dp, tween(300, easing = CubicBezierEasing(0.3f, 1.4f, 0.5f, 1f)), label = "tx")
    Box(Modifier.width(52.dp).height(32.dp).background(track, RoundedCornerShape(16.dp)).padding(3.dp)) {
        Box(Modifier.offset(x = x).size(26.dp).background(Palette.White, CircleShape))
    }
}

// ------------------------------------------------------------------ mini-lecteur et navigation

@Composable
private fun MiniPlayer(ui: UiState, vm: MainViewModel, accent: Color, onCast: () -> Unit) {
    val t = ui.current ?: return
    Box(
        Modifier.padding(horizontal = 12.dp).fillMaxWidth().height(64.dp)
            .shadow(16.dp, RoundedCornerShape(18.dp)).clip(RoundedCornerShape(18.dp)).background(accent)
    ) {
        Row(Modifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
            Row(
                Modifier.weight(1f).fillMaxSize().clickable { vm.openSheet(true) }.padding(start = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Cover(t, 44.dp, 6.dp, Modifier.border(1.dp, Color.White.copy(alpha = 0.25f), RoundedCornerShape(6.dp)))
                Column(Modifier.weight(1f)) {
                    Text(t.title, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, color = Palette.White,
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(t.subtitle, fontFamily = Body, fontSize = 13.sp, color = Palette.White.copy(alpha = 0.85f),
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            CastButton(vm, Modifier.size(48.dp, 64.dp), onCast)
            Box(Modifier.size(56.dp, 64.dp).clickable { vm.togglePlay() }, contentAlignment = Alignment.Center) {
                Icon(if (ui.playing) Icons.Pause else Icons.Play, if (ui.playing) "Pause" else "Lecture",
                    tint = Palette.White, modifier = Modifier.size(24.dp))
            }
        }
        val f = if (ui.duration > 0) ui.position.toFloat() / ui.duration else 0f
        Box(Modifier.align(Alignment.BottomStart).fillMaxWidth(f.coerceIn(0f, 1f)).height(3.dp)
            .background(Palette.White.copy(alpha = 0.95f)))
    }
}

@Composable
private fun BottomNav(screen: Screen, onPick: (Screen) -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(Palette.Black).navigationBarsPadding().height(72.dp).padding(top = 8.dp),
        horizontalArrangement = Arrangement.SpaceAround,
    ) {
        listOf(
            Screen.RELEASES to ("Sorties" to Icons.Disc),
            Screen.LIBRARY to ("Bibliothèque" to Icons.Library),
            Screen.OFFLINE to ("Hors ligne" to Icons.Offline),
            Screen.SHARES to ("Partages" to Icons.Share),
        )
            .forEach { (s, p) ->
                val c by animateColorAsState(if (s == screen) Palette.White else Palette.Dim, tween(250), label = "nav")
                Column(
                    Modifier.weight(1f).padding(horizontal = 4.dp).height(52.dp).clip(RoundedCornerShape(12.dp)).clickable { onPick(s) },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(3.dp),
                ) {
                    Icon(p.second, null, tint = c, modifier = Modifier.size(24.dp))
                    Text(p.first, color = c, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 12.sp)
                }
            }
    }
}

// ------------------------------------------------------------------ lecteur plein écran

@Composable
private fun PlayerSheet(ui: UiState, vm: MainViewModel, accent: Color, onCast: () -> Unit) {
    val t = ui.current
    BackHandler(ui.sheetOpen) { vm.openSheet(false) }
    AnimatedVisibility(
        ui.sheetOpen && t != null,
        enter = slideInVertically(tween(520, easing = Ease)) { it },
        exit = slideOutVertically(tween(420, easing = Ease)) { it },
    ) {
        if (t == null) return@AnimatedVisibility
        val bg = deepOf(accent)
        val coverScale = remember { Animatable(0.72f) }
        LaunchedEffect(Unit) { delay(60); coverScale.animateTo(1f, tween(600, easing = Ease)) }
        var dragY by remember { mutableStateOf(0f) }
        val scope = rememberCoroutineScope()
        val spin = rememberInfiniteTransition(label = "spin")
        val angle by spin.animateFloat(0f, 360f, infiniteRepeatable(tween(8000, easing = LinearEasing)), label = "a")
        var frozenAngle by remember { mutableStateOf(0f) }
        if (ui.playing) frozenAngle = angle

        Column(
            Modifier.fillMaxSize()
                .graphicsLayer { translationY = dragY.coerceAtLeast(0f) }
                .background(bg)
                .clickable(remember { MutableInteractionSource() }, null) {}
                .pointerInput(Unit) {
                    detectVerticalDragGestures(
                        onDragEnd = {
                            if (dragY > 160.dp.toPx()) vm.openSheet(false)
                            scope.launch { val a = Animatable(dragY); a.animateTo(0f, tween(250)) { dragY = value } }
                        },
                    ) { _, d -> dragY += d }
                }
                .statusBarsPadding().navigationBarsPadding()
                .padding(start = 24.dp, end = 24.dp, top = 12.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier.size(44.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.12f))
                        .clickable { vm.openSheet(false) },
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.ChevronDown, "Fermer le lecteur", tint = Palette.White, modifier = Modifier.size(22.dp)) }
                Text(t.release?.title ?: t.statusLabel, fontFamily = Body, fontSize = 14.sp, color = Palette.White.copy(alpha = 0.85f),
                    maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f).padding(horizontal = 12.dp),
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center)
                Box(
                    Modifier.size(44.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.12f))
                        .clickable { vm.openShare(ShareRequest("tracks", tracks = listOf(t.id to t.title), label = t.title)) },
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Share, "Partager ce morceau", tint = Palette.White, modifier = Modifier.size(20.dp)) }
                Spacer(Modifier.width(8.dp))
                CastButton(vm, Modifier.size(44.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.12f)), onCast)
            }

            BoxWithConstraints(Modifier.fillMaxWidth().weight(1f, fill = false), contentAlignment = Alignment.Center) {
                val side = minOf(maxWidth, maxHeight)
                Box(
                    Modifier.size(side).graphicsLayer { scaleX = coverScale.value; scaleY = coverScale.value }
                        .shadow(30.dp, RoundedCornerShape(10.dp)),
                    contentAlignment = Alignment.Center,
                ) {
                    Cover(t, side, 10.dp, hiRes = true)
                    // anneau qui tourne pendant la lecture
                    Canvas(Modifier.size(side * 0.88f).rotate(frozenAngle)) {
                        val r = size.minDimension / 2
                        drawCircle(Color.White.copy(alpha = 0.18f), r, style = androidx.compose.ui.graphics.drawscope.Stroke(1.dp.toPx()))
                        drawCircle(Color.White.copy(alpha = 0.7f), 3.dp.toPx(), center = Offset(size.width / 2, size.height / 2 - r))
                    }
                }
            }

            Column {
                Text(t.title, fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 30.sp, lineHeight = 32.sp,
                    letterSpacing = (-0.8).sp, color = Palette.White, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(t.subtitle, fontFamily = Body, fontSize = 16.sp, color = Palette.White.copy(alpha = 0.8f),
                    modifier = Modifier.padding(top = 4.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
                // « EP · In My Flesh » : ouvre la fiche de la sortie
                val r = t.release
                if (r != null && r.id > 0) Row(
                    Modifier.padding(top = 10.dp).height(30.dp).clip(RoundedCornerShape(15.dp))
                        .background(Color.White.copy(alpha = 0.14f)).clickable { vm.openRelease(r.id) }
                        .padding(horizontal = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Box(Modifier.size(8.dp).background(typeColor(r.type), CircleShape))
                    Text(listOf(r.typeLabel, r.title).filter { it.isNotBlank() }.joinToString(" · "),
                        fontFamily = Body, fontSize = 14.sp, color = Palette.White, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }

            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                t.bpm?.let { InfoPill("${it.roundToInt()} BPM") }
                CamelotPill(t, withKey = true)
                t.lufs?.let { InfoPill("%.1f LUFS".format(it)) }
            }

            Column {
                val f = if (ui.duration > 0) ui.position.toFloat() / ui.duration else 0f
                WaveformSeek(ui.waveform, f, vm::seekFraction)
                Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
                    Text(formatTime(ui.position), fontFamily = Body, fontSize = 13.sp, color = Palette.White.copy(alpha = 0.75f),
                        modifier = Modifier.weight(1f))
                    Text(formatTime(ui.duration), fontFamily = Body, fontSize = 13.sp, color = Palette.White.copy(alpha = 0.75f))
                }
            }

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(56.dp).clip(CircleShape).clickable { vm.previous() }, contentAlignment = Alignment.Center) {
                    Icon(Icons.Prev, "Morceau précédent", tint = Palette.White, modifier = Modifier.size(30.dp))
                }
                val press = remember { MutableInteractionSource() }
                Box(
                    Modifier.size(84.dp).clip(CircleShape).background(Palette.White).clickable(press, null) { vm.togglePlay() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(if (ui.playing) Icons.Pause else Icons.Play, if (ui.playing) "Pause" else "Lecture",
                        tint = bg, modifier = Modifier.size(32.dp))
                }
                Box(Modifier.size(56.dp).clip(CircleShape).clickable { vm.next() }, contentAlignment = Alignment.Center) {
                    Icon(Icons.Next, "Morceau suivant", tint = Palette.White, modifier = Modifier.size(30.dp))
                }
            }

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Pulse, null, tint = Palette.White.copy(alpha = 0.85f), modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(8.dp))
                val castTarget by vm.cast.target.collectAsState()
                val label = when {
                    castTarget != null -> "Sur ${castTarget!!.name} · ${t.qualityLabel}"
                    ui.offlinePlayback -> "Hors ligne · ${t.qualityLabel}"
                    ui.playingQuality == Quality.HIGH -> "Opus 256 kb/s · données mobiles"
                    ui.playingQuality == Quality.ECO -> "Opus 128 kb/s · données mobiles"
                    else -> "${t.qualityLabel} · FLAC 24 bits"
                }
                Text(label, fontFamily = Body, fontSize = 14.sp, color = Palette.White.copy(alpha = 0.85f))
            }
        }
    }
}

// ------------------------------------------------------------------ diffusion (Mu-so)

/** Icône « Diffuser » : blanche pendant la diffusion, grisée hors Wi-Fi */
@Composable
private fun CastButton(vm: MainViewModel, modifier: Modifier, onClick: () -> Unit) {
    val target by vm.cast.target.collectAsState()
    val connection by vm.connection.collectAsState()
    val wifi = remember(connection) { vm.onWifi() }
    Box(modifier.clickable(onClick = onClick), contentAlignment = Alignment.Center) {
        Icon(Icons.Cast, if (target != null) "Diffusion sur ${target!!.name}" else "Diffuser",
            tint = when {
                target != null -> Palette.White
                wifi -> Palette.White.copy(alpha = 0.8f)
                else -> Palette.White.copy(alpha = 0.35f)
            },
            modifier = Modifier.size(22.dp))
        if (target != null) Box(Modifier.align(Alignment.TopEnd).padding(top = 12.dp, end = 10.dp).size(7.dp)
            .background(Color(0xFF6BE38A), CircleShape))
    }
}

/** Feuille « Diffuser sur… » : ce téléphone, puis les lecteurs réseau du Wi-Fi */
@Composable
private fun CastSheet(open: Boolean, vm: MainViewModel, onClose: () -> Unit, onMessage: (String) -> Unit) {
    BackHandler(open) { onClose() }
    val devices by vm.cast.devices.collectAsState()
    val searching by vm.cast.searching.collectAsState()
    val target by vm.cast.target.collectAsState()
    val wifi = remember(open) { vm.onWifi() }
    LaunchedEffect(open) { if (open && wifi) vm.cast.search() }
    AnimatedVisibility(open, enter = fadeIn(tween(200)), exit = fadeOut(tween(200))) {
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.55f))
                .clickable(remember { MutableInteractionSource() }, null) { onClose() },
            contentAlignment = Alignment.BottomCenter,
        ) {
            Column(
                Modifier.fillMaxWidth().background(Palette.Surface, RoundedCornerShape(topStart = 22.dp, topEnd = 22.dp))
                    .clickable(remember { MutableInteractionSource() }, null) {}
                    .navigationBarsPadding().padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text("Diffuser sur…", fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 24.sp, color = Palette.White,
                    modifier = Modifier.padding(bottom = 8.dp))
                CastRow("Ce téléphone", null, selected = target == null) { vm.stopCasting(); onClose() }
                if (!wifi) Text("Wi-Fi maison requis : la diffusion vers une enceinte réseau ne marche qu'à la maison.",
                    fontFamily = Body, fontSize = 14.sp, color = Palette.Muted, modifier = Modifier.padding(top = 8.dp))
                else {
                    devices.forEach { d ->
                        CastRow(d.name, "Lecteur réseau · FLAC sans perte", selected = target?.udn == d.udn) {
                            val err = vm.castTo(d)
                            if (err != null) onMessage(err) else { onMessage("Diffusion sur ${d.name}"); onClose() }
                        }
                    }
                    when {
                        searching -> Text("Recherche des appareils…", fontFamily = Body, fontSize = 14.sp, color = Palette.Muted,
                            modifier = Modifier.padding(top = 8.dp))
                        devices.isEmpty() -> Text(
                            "Aucun appareil trouvé — vérifie que le téléphone est sur le Wi-Fi de la maison.",
                            fontFamily = Body, fontSize = 14.sp, color = Palette.Muted, modifier = Modifier.padding(top = 8.dp))
                    }
                    if (!searching) Text("Rechercher à nouveau", fontFamily = Body, fontWeight = FontWeight.SemiBold,
                        fontSize = 15.sp, color = Palette.White,
                        modifier = Modifier.padding(top = 10.dp).clip(RoundedCornerShape(18.dp))
                            .background(Color.White.copy(alpha = 0.12f)).clickable { vm.cast.search() }
                            .padding(horizontal = 16.dp, vertical = 10.dp))
                }
            }
        }
    }
}

@Composable
private fun CastRow(name: String, detail: String?, selected: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable(onClick = onClick).padding(vertical = 10.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(if (detail == null) Icons.Pulse else Icons.Cast, null, tint = Palette.White, modifier = Modifier.size(22.dp))
        Column(Modifier.weight(1f)) {
            Text(name, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, color = Palette.White)
            if (detail != null) Text(detail, fontFamily = Body, fontSize = 13.sp, color = Palette.Muted)
        }
        if (selected) Icon(Icons.Check, "Sélectionné", tint = Palette.White, modifier = Modifier.size(20.dp))
    }
}

@Composable
internal fun InfoPill(text: String) {
    Box(
        Modifier.height(30.dp).background(Color.White.copy(alpha = 0.14f), RoundedCornerShape(15.dp)).padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) { Text(text, fontFamily = Body, fontSize = 14.sp, color = Palette.White) }
}
