package com.otoko.mobile.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.otoko.mobile.MainViewModel
import com.otoko.mobile.Share
import com.otoko.mobile.ShareRequest
import com.otoko.mobile.Track
import com.otoko.mobile.UiState
import kotlinx.coroutines.delay
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId

private val KIND_LABEL = mapOf("discography" to "Discographie", "releases" to "Sorties", "tracks" to "Morceaux")
private val DL_QUALITY = listOf("original" to "Fichier d'origine", "lossless" to "FLAC sans perte", "high" to "AAC 256 kb/s")
private val STATE = mapOf(
    "active" to ("Actif" to Color(0xFF59C17A)),
    "expired" to ("Expiré" to Color(0xFF8F8F9A)),
    "revoked" to ("Désactivé" to Color(0xFFFF7A7A)),
)
private val VALIDITY = listOf<Pair<Int?, String>>(
    1 to "1 jour", 7 to "7 jours", 30 to "30 jours", 90 to "3 mois", 365 to "1 an", null to "Sans limite")

private const val FUNNEL_WARNING =
    "Le partage public n'est pas encore allumé : clique sur « Partage public… » dans la page Mobile et partage, sur ton PC."

private fun plural(n: Int, word: String): String {
    val w = when {
        n <= 1 -> word
        word.endsWith("eau") -> word + "x"
        else -> word + "s"
    }
    return "$n $w"
}

/** "2026-12-02T10:00:00Z" → heure du téléphone */
private fun localTime(iso: String?) =
    iso?.let { runCatching { Instant.parse(it).atZone(ZoneId.systemDefault()) }.getOrNull() }

private fun sendLink(context: Context, url: String, title: String) {
    val i = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, url)
        putExtra(Intent.EXTRA_SUBJECT, title)
    }
    runCatching { context.startActivity(Intent.createChooser(i, "Envoyer le lien")) }
}

private fun copyLink(context: Context, url: String) {
    context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("Lien Otoko Vault", url))
}

// ------------------------------------------------------------------ onglet Partages

@Composable
fun SharesScreen(ui: UiState, vm: MainViewModel, accent: Color, onMessage: (String) -> Unit) {
    val context = LocalContext.current
    var toDelete by remember { mutableStateOf<Share?>(null) }

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 110.dp)) {
        item(key = "head") {
            Column(
                Modifier.fillMaxWidth().background(accent).statusBarsPadding()
                    .padding(start = 20.dp, end = 20.dp, top = 14.dp, bottom = 18.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text("Partages", fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 44.sp, lineHeight = 42.sp,
                        letterSpacing = (-1.5).sp, color = Palette.White, modifier = Modifier.weight(1f))
                    PillButton("Nouveau", Icons.Plus, primary = true) { vm.openShare(ShareRequest("releases")) }
                }
                Text("Des liens privés pour faire écouter (ou télécharger) ce que tu choisis, à qui tu veux, même sans Tailscale.",
                    fontFamily = Body, fontSize = 14.sp, color = Palette.White.copy(alpha = 0.85f))
            }
        }
        if (ui.sharesError != null) item(key = "err") {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(ui.sharesError, fontFamily = Body, fontSize = 15.sp, color = Palette.Muted)
                PillButton("Réessayer", Icons.Retry) { vm.loadShares() }
            }
        } else {
            if (!ui.funnel) item(key = "funnel") { FunnelWarning(Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp)) }
            if (ui.sharesLoading && ui.shares.isEmpty()) item(key = "load") {
                Text("Chargement…", fontFamily = Body, fontSize = 15.sp, color = Palette.Muted, modifier = Modifier.padding(20.dp))
            } else if (ui.shares.isEmpty()) item(key = "vide") {
                Text("Aucun partage pour l'instant. Ouvre une sortie ou un morceau et touche « Partager », ou crée un lien ici.",
                    fontFamily = Body, fontSize = 15.sp, color = Palette.Muted, modifier = Modifier.padding(20.dp))
            }
        }
        items(ui.shares, key = { "sh-${it.id}" }) { s ->
            ShareCard(
                s, ui,
                onSend = { sendLink(context, s.url, s.label.ifBlank { "Écoute privée" }) },
                onCopy = { copyLink(context, s.url); onMessage("Lien copié") },
                onAction = { a -> if (a == "delete") toDelete = s else vm.shareAction(s, a, onMessage) },
            )
        }
    }

    toDelete?.let { s ->
        AlertDialog(
            onDismissRequest = { toDelete = null },
            title = { Text("Supprimer ce lien ?") },
            text = { Text("Il ne marchera plus du tout.") },
            confirmButton = {
                TextButton(onClick = { toDelete = null; vm.shareAction(s, "delete", onMessage) }) { Text("Supprimer") }
            },
            dismissButton = { TextButton(onClick = { toDelete = null }) { Text("Annuler") } },
        )
    }
}

@Composable
private fun ShareCard(s: Share, ui: UiState, onSend: () -> Unit, onCopy: () -> Unit, onAction: (String) -> Unit) {
    val what = when (s.kind) {
        "discography" -> "Toute la discographie"
        "releases" -> s.items.joinToString(", ") { id -> ui.releases.firstOrNull { it.id == id }?.title ?: "Sortie supprimée" }
        else -> plural(s.items.size, "morceau")
    }
    val (stateLabel, stateColor) = STATE[s.state] ?: STATE.getValue("active")
    val quality = DL_QUALITY.firstOrNull { it.first == s.downloadQuality }?.second ?: s.downloadQuality
    val rights = if (s.canDownload)
        "Écoute + téléchargement ($quality" + (s.maxDownloads?.let { ", ${s.downloads}/$it" } ?: "") + ")"
    else "Écoute seule"
    val exp = localTime(s.expiresAt)?.let {
        (if (s.state == "expired") "A expiré le " else "Jusqu'au ") + formatDateFr(it.toLocalDate().toString())
    } ?: "Sans date limite"
    val last = localTime(s.lastAccess)?.let {
        " · dernière visite le " + formatDateFr(it.toLocalDate().toString()) + " à %02d:%02d".format(it.hour, it.minute)
    } ?: ""

    Column(
        Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp).fillMaxWidth()
            .background(Palette.Surface, RoundedCornerShape(16.dp)).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(s.label.ifBlank { what }, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 17.sp,
                color = Palette.White, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            Box(
                Modifier.height(22.dp).background(stateColor, RoundedCornerShape(11.dp)).padding(horizontal = 8.dp),
                contentAlignment = Alignment.Center,
            ) { Text(stateLabel, color = Palette.Black, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 12.sp) }
        }
        Text("${KIND_LABEL[s.kind] ?: s.kind} · $what", fontFamily = Body, fontSize = 14.sp, color = Palette.Muted,
            maxLines = 2, overflow = TextOverflow.Ellipsis)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            if (s.hasPassword) Icon(Icons.Lock, "Mot de passe", tint = Palette.White, modifier = Modifier.size(15.dp))
            Text("$rights · $exp" + if (s.hasPassword) " · mot de passe" else "",
                fontFamily = Body, fontSize = 14.sp, color = Palette.Muted)
        }
        Text("${plural(s.views, "visite")} · ${plural(s.plays, "écoute")} · ${plural(s.downloads, "téléchargement")}$last",
            fontFamily = Body, fontSize = 13.sp, color = Palette.Dim)
        Row(
            Modifier.padding(top = 10.dp).horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (s.state == "active") {
                PillButton("Envoyer", Icons.Share, small = true, onClick = onSend)
                PillButton("Copier le lien", Icons.Copy, small = true, onClick = onCopy)
                PillButton("Désactiver", small = true) { onAction("off") }
            }
            if (s.state == "revoked") PillButton("Réactiver", small = true) { onAction("on") }
            if (s.state == "expired") PillButton("Prolonger 30 jours", small = true) { onAction("extend") }
            PillButton("Supprimer", Icons.Trash, small = true) { onAction("delete") }
        }
    }
}

@Composable
private fun FunnelWarning(modifier: Modifier = Modifier) {
    Text(FUNNEL_WARNING, fontFamily = Body, fontSize = 14.sp, lineHeight = 20.sp, color = Palette.Black,
        modifier = modifier.fillMaxWidth().background(Color(0xFFF5B942), RoundedCornerShape(12.dp)).padding(14.dp))
}

@Composable
private fun PillButton(
    label: String, icon: androidx.compose.ui.graphics.vector.ImageVector? = null,
    primary: Boolean = false, small: Boolean = false, onClick: () -> Unit,
) {
    val h = if (small) 38.dp else 44.dp
    Row(
        Modifier.height(h).clip(RoundedCornerShape(h / 2))
            .background(if (primary) Palette.White else Color.White.copy(alpha = 0.12f))
            .clickable(onClick = onClick).padding(horizontal = if (small) 14.dp else 18.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val fg = if (primary) Palette.Black else Palette.White
        if (icon != null) Icon(icon, null, tint = fg, modifier = Modifier.size(if (small) 16.dp else 18.dp))
        Text(label, color = fg, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = if (small) 14.sp else 15.sp,
            maxLines = 1)
    }
}

// ------------------------------------------------------------------ feuille « Partager »

@Composable
@OptIn(ExperimentalLayoutApi::class, ExperimentalFoundationApi::class)
fun ShareSheet(ui: UiState, vm: MainViewModel, onMessage: (String) -> Unit) {
    val req = ui.shareRequest
    BackHandler(req != null) { vm.closeShare() }
    AnimatedVisibility(req != null, enter = fadeIn(tween(200)), exit = fadeOut(tween(200))) {
        // garde le contenu pendant l'animation de sortie
        val last = remember { mutableStateOf(req) }
        if (req != null) last.value = req
        val r = last.value ?: return@AnimatedVisibility
        val context = LocalContext.current

        var kind by remember(r) { mutableStateOf(r.kind) }
        val releases = remember(r) { mutableStateListOf(*r.releases.toTypedArray()) }
        val tracks = remember(r) { mutableStateListOf(*r.tracks.toTypedArray()) }
        var label by remember(r) { mutableStateOf(r.label) }
        var message by remember(r) { mutableStateOf("") }
        var canDownload by remember(r) { mutableStateOf(false) }
        var quality by remember(r) { mutableStateOf("original") }
        var maxDl by remember(r) { mutableStateOf("") }
        var validity by remember(r) { mutableStateOf<Int?>(30) }
        var password by remember(r) { mutableStateOf("") }
        var error by remember(r) { mutableStateOf<String?>(null) }
        var busy by remember(r) { mutableStateOf(false) }
        var created by remember(r) { mutableStateOf<Pair<Share, Boolean>?>(null) }

        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.55f))
                .clickable(remember { MutableInteractionSource() }, null) { vm.closeShare() },
            contentAlignment = Alignment.BottomCenter,
        ) {
            BoxWithConstraints(Modifier.fillMaxWidth().statusBarsPadding().imePadding(), contentAlignment = Alignment.BottomCenter) {
                Column(
                    Modifier.fillMaxWidth().heightIn(max = maxHeight)
                        .background(Palette.Surface, RoundedCornerShape(topStart = 22.dp, topEnd = 22.dp))
                        .clickable(remember { MutableInteractionSource() }, null) {}
                        .verticalScroll(rememberScrollState())
                        .navigationBarsPadding().padding(20.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    val done = created
                    if (done != null) {
                        val (s, funnel) = done
                        Text("Lien prêt", fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 26.sp, color = Palette.White)
                        Text("Envoie-le à qui tu veux." + if (password.isNotEmpty()) " N'oublie pas de donner le mot de passe à part." else "",
                            fontFamily = Body, fontSize = 14.sp, color = Palette.Muted)
                        SelectionContainer {
                            Text(s.url, fontFamily = Body, fontSize = 15.sp, color = Palette.White,
                                modifier = Modifier.fillMaxWidth().background(Color.White.copy(alpha = 0.08f), RoundedCornerShape(12.dp))
                                    .padding(14.dp))
                        }
                        if (!funnel) FunnelWarning()
                        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            PillButton("Envoyer", Icons.Share, primary = true) { sendLink(context, s.url, s.label.ifBlank { "Écoute privée" }) }
                            PillButton("Copier", Icons.Copy) { copyLink(context, s.url); onMessage("Lien copié") }
                            Spacer(Modifier.weight(1f))
                            PillButton("Fermer") { vm.closeShare() }
                        }
                        return@Column
                    }

                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("Partager", fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 26.sp, color = Palette.White,
                            modifier = Modifier.weight(1f))
                        Box(
                            Modifier.size(40.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.12f)).clickable { vm.closeShare() },
                            contentAlignment = Alignment.Center,
                        ) { Icon(Icons.Close, "Fermer", tint = Palette.White, modifier = Modifier.size(18.dp)) }
                    }

                    Segmented(listOf("releases" to "Sorties", "tracks" to "Morceaux", "discography" to "Discographie"), kind) { kind = it }

                    // ce qui est partagé
                    if (kind == "discography") {
                        Hint("Toutes tes sorties déjà parues. La liste suit ta Carrière : une nouvelle sortie apparaît toute seule.")
                    } else {
                        val picked = if (kind == "releases") releases else tracks
                        if (picked.isEmpty()) Hint("Rien de choisi pour l'instant.")
                        else FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            picked.toList().forEach { (id, title) ->
                                Row(
                                    Modifier.height(34.dp).background(Color.White.copy(alpha = 0.14f), RoundedCornerShape(17.dp))
                                        .padding(start = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(title, fontFamily = Body, fontSize = 14.sp, color = Palette.White, maxLines = 1,
                                        overflow = TextOverflow.Ellipsis, modifier = Modifier.widthIn(max = 220.dp))
                                    Box(
                                        Modifier.size(34.dp).clip(CircleShape).clickable { picked.removeAll { it.first == id } },
                                        contentAlignment = Alignment.Center,
                                    ) { Icon(Icons.Close, "Retirer $title", tint = Palette.White, modifier = Modifier.size(14.dp)) }
                                }
                            }
                        }
                        if (kind == "releases") AddRelease(ui, releases) else AddTrack(vm, tracks)
                    }

                    Field("Titre du partage", label, { label = it.take(120) }, "Ex. : Pour le label X")
                    Field("Message (facultatif)", message, { message = it.take(1000) }, "Un mot pour la personne qui reçoit le lien",
                        singleLine = false)

                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        FieldLabel("Droits")
                        Segmented(listOf("0" to "Écoute seule", "1" to "Écoute + téléchargement"), if (canDownload) "1" else "0") {
                            canDownload = it == "1"
                        }
                    }
                    if (canDownload) {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            FieldLabel("Qualité")
                            Segmented(listOf("original" to "Origine", "lossless" to "FLAC", "high" to "AAC 256"), quality) { quality = it }
                            Hint(DL_QUALITY.first { it.first == quality }.second + (if (quality == "high") " (léger)" else ""))
                        }
                        Field("Téléchargements max", maxDl, { v -> maxDl = v.filter { it.isDigit() }.take(5) }, "Illimité",
                            keyboard = KeyboardType.Number)
                    }

                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        FieldLabel("Valable")
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            VALIDITY.forEach { (days, text) -> Choice(text, validity == days) { validity = days } }
                        }
                    }
                    Field("Mot de passe (facultatif)", password, { password = it }, "Aucun")

                    error?.let { Text(it, fontFamily = Body, fontSize = 14.sp, color = Color(0xFFFF8A80)) }

                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Spacer(Modifier.weight(1f))
                        PillButton("Annuler") { vm.closeShare() }
                        PillButton(if (busy) "Création…" else "Créer le lien", Icons.Share, primary = true) {
                            if (busy) return@PillButton
                            val items = when (kind) {
                                "releases" -> releases.map { it.first }
                                "tracks" -> tracks.map { it.first }
                                else -> emptyList()
                            }
                            if (kind != "discography" && items.isEmpty()) {
                                error = "Choisis au moins un morceau ou une sortie."; return@PillButton
                            }
                            val body = JSONObject().apply {
                                put("kind", kind)
                                put("items", JSONArray(items))
                                put("label", label.trim().ifBlank { if (kind == "discography") "Discographie" else "" })
                                put("message", message)
                                put("can_download", canDownload)
                                put("download_quality", quality)
                                put("max_downloads", if (canDownload) maxDl.toIntOrNull() ?: JSONObject.NULL else JSONObject.NULL)
                                put("expires_days", validity ?: JSONObject.NULL)
                                put("password", password)
                            }
                            busy = true; error = null
                            vm.createShare(body, onDone = { s, funnel -> busy = false; created = s to funnel },
                                onError = { busy = false; error = it })
                        }
                    }
                }
            }
        }
    }
}

/** « Ajouter une sortie » : liste des sorties pas encore choisies */
@Composable
private fun AddRelease(ui: UiState, picked: MutableList<Pair<Long, String>>) {
    var open by remember { mutableStateOf(false) }
    val left = ui.releases.filter { r -> picked.none { it.first == r.id } }
    Box {
        PillButton("Ajouter une sortie", Icons.Plus, small = true) { open = true }
        DropdownMenu(open, { open = false }, modifier = Modifier.background(Palette.Surface).heightIn(max = 360.dp)) {
            if (left.isEmpty()) DropdownMenuItem(
                text = { Text(if (ui.releasesLoading) "Chargement…" else "Aucune autre sortie", fontFamily = Body, color = Palette.Muted) },
                onClick = { open = false })
            left.forEach { r ->
                DropdownMenuItem(
                    text = {
                        Column {
                            Text(r.title, fontFamily = Body, fontSize = 16.sp, color = Palette.White, maxLines = 1,
                                overflow = TextOverflow.Ellipsis)
                            Text(r.typeLabel + if (r.upcoming) ", à venir" else "", fontFamily = Body, fontSize = 13.sp,
                                color = Palette.Muted)
                        }
                    },
                    onClick = { open = false; picked.add(r.id to r.title) })
            }
        }
    }
}

/** « Ajouter un morceau » : recherche dans toute la bibliothèque */
@Composable
private fun AddTrack(vm: MainViewModel, picked: MutableList<Pair<Long, String>>) {
    var q by remember { mutableStateOf("") }
    var results by remember { mutableStateOf<List<Track>>(emptyList()) }
    LaunchedEffect(q) {
        if (q.isBlank()) { results = emptyList(); return@LaunchedEffect }
        delay(250)
        results = vm.searchTracks(q.trim())
    }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Field("Ajouter un morceau", q, { q = it }, "Rechercher un titre")
        results.filter { t -> picked.none { it.first == t.id } }.forEach { t ->
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { picked.add(t.id to t.title) }
                    .padding(vertical = 8.dp, horizontal = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Cover(t, 40.dp, 6.dp)
                Column(Modifier.weight(1f)) {
                    Text(t.title, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, color = Palette.White,
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(t.release?.title ?: t.natureLabel, fontFamily = Body, fontSize = 13.sp, color = Palette.Muted,
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                Icon(Icons.Plus, "Ajouter", tint = Palette.White, modifier = Modifier.size(20.dp))
            }
        }
    }
}

@Composable
private fun FieldLabel(text: String) {
    Text(text, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, color = Palette.White)
}

@Composable
private fun Hint(text: String) {
    Text(text, fontFamily = Body, fontSize = 14.sp, color = Palette.Muted)
}

@Composable
private fun Field(
    label: String, value: String, onChange: (String) -> Unit, placeholder: String,
    singleLine: Boolean = true, keyboard: KeyboardType = KeyboardType.Text,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FieldLabel(label)
        Box(
            Modifier.fillMaxWidth().heightIn(min = 46.dp).background(Color.White.copy(alpha = 0.08f), RoundedCornerShape(12.dp))
                .padding(horizontal = 14.dp, vertical = 12.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (value.isEmpty()) Text(placeholder, color = Palette.Dim, fontFamily = Body, fontSize = 16.sp, maxLines = 1)
            BasicTextField(
                value, onChange, singleLine = singleLine, minLines = if (singleLine) 1 else 2,
                cursorBrush = SolidColor(Palette.White),
                keyboardOptions = KeyboardOptions(keyboardType = keyboard),
                textStyle = TextStyle(color = Palette.White, fontFamily = Body, fontSize = 16.sp),
                modifier = Modifier.fillMaxWidth())
        }
    }
}

/** Choix exclusif sur une ligne (Sorties / Morceaux / Discographie…) */
@Composable
private fun Segmented(options: List<Pair<String, String>>, value: String, onPick: (String) -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(46.dp).background(Color.White.copy(alpha = 0.08f), RoundedCornerShape(14.dp)).padding(4.dp),
    ) {
        options.forEach { (k, text) ->
            val on = k == value
            val bg by animateColorAsState(if (on) Palette.White else Color.Transparent, tween(250), label = "seg")
            val fg by animateColorAsState(if (on) Palette.Black else Palette.White, tween(250), label = "segfg")
            Box(
                Modifier.weight(1f).fillMaxSize().clip(RoundedCornerShape(10.dp)).background(bg).clickable { onPick(k) },
                contentAlignment = Alignment.Center,
            ) {
                Text(text, color = fg, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 14.sp, maxLines = 1,
                    overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun Choice(text: String, on: Boolean, onClick: () -> Unit) {
    val bg by animateColorAsState(if (on) Palette.White else Color.White.copy(alpha = 0.10f), tween(250), label = "ch")
    val fg by animateColorAsState(if (on) Palette.Black else Palette.White, tween(250), label = "chfg")
    Box(
        Modifier.height(36.dp).clip(RoundedCornerShape(18.dp)).background(bg).clickable(onClick = onClick).padding(horizontal = 14.dp),
        contentAlignment = Alignment.Center,
    ) { Text(text, color = fg, fontFamily = Body, fontWeight = FontWeight.Medium, fontSize = 14.sp) }
}
