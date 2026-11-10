package com.otoko.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.otoko.mobile.MainViewModel
import com.otoko.mobile.Note
import com.otoko.mobile.STATUSES
import com.otoko.mobile.UiState

/**
 * Sous le morceau en cours : garder, rejeter, son statut, et une note à l'instant où on l'entend. Tout part vers ton PC
 * (ou attend qu'il réponde) et se retrouve dans le logiciel : la note devient un repère sur la forme d'onde.
 */
@Composable
internal fun MineRow(ui: UiState, vm: MainViewModel, onMessage: (String) -> Unit) {
    val t = ui.current ?: return
    val mine = ui.mine?.takeIf { it.trackId == t.id }
    val pick = mine?.pick ?: t.pick
    val status = mine?.status ?: t.status
    var statusOpen by remember { mutableStateOf(false) }
    // L'instant de la note est pris à l'ouverture : on écrit pendant que la musique continue.
    var noteAt by remember { mutableStateOf<Long?>(null) }

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        MineChip("Garder", pick == 1) { vm.setPick(if (pick == 1) 0 else 1, onMessage) }
        MineChip("Rejeter", pick == -1) { vm.setPick(if (pick == -1) 0 else -1, onMessage) }
        Box(Modifier.weight(1f)) {
            MineChip(STATUSES.firstOrNull { it.first == status }?.second ?: "Statut", false, Modifier.fillMaxWidth()) { statusOpen = true }
            DropdownMenu(statusOpen, onDismissRequest = { statusOpen = false }) {
                STATUSES.forEach { (key, label) ->
                    DropdownMenuItem(
                        text = { Text(label, fontWeight = if (key == status) FontWeight.Bold else FontWeight.Normal) },
                        onClick = { statusOpen = false; if (key != status) vm.setStatus(key, onMessage) },
                    )
                }
            }
        }
        val n = mine?.notes?.size ?: 0
        MineChip(if (n > 0) "Note · $n" else "Note", false) { noteAt = ui.position }
    }

    noteAt?.let { at ->
        NoteDialog(
            atMs = at,
            notes = mine?.notes.orEmpty(),
            onSeek = { note -> if (ui.duration > 0) vm.seekFraction((note.time * 1000 / ui.duration).toFloat()) },
            onRemove = { note -> vm.removeNote(note, onMessage) },
            onDone = { text -> noteAt = null; if (text != null) vm.addNote(at, text, onMessage) },
        )
    }
}

@Composable
private fun MineChip(label: String, on: Boolean, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Box(
        modifier.height(40.dp).clip(RoundedCornerShape(20.dp))
            .background(if (on) Palette.White else Color.White.copy(alpha = 0.14f))
            .clickable(onClick = onClick)
            .semantics { role = Role.Button; selected = on }
            .padding(horizontal = 16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 14.sp,
            color = if (on) Palette.Black else Palette.White, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Écrire (ou dicter avec le micro du clavier) une note à cet instant ; les notes déjà posées, pour y aller ou les retirer */
@Composable
private fun NoteDialog(
    atMs: Long, notes: List<Note>, onSeek: (Note) -> Unit, onRemove: (Note) -> Unit, onDone: (String?) -> Unit,
) {
    var text by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = { onDone(null) },
        title = { Text("Note à ${formatTime(atMs)}") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    text, { text = it.take(300) }, Modifier.fillMaxWidth(),
                    placeholder = { Text("Ex. : kick trop fort, voix en avant…") },
                    supportingText = { Text("Vide : un simple repère.") },
                )
                if (notes.isNotEmpty()) Column(Modifier.heightIn(max = 220.dp).verticalScroll(rememberScrollState())) {
                    notes.forEach { n ->
                        Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(formatTime((n.time * 1000).toLong()), fontWeight = FontWeight.Bold,
                                modifier = Modifier.width(56.dp).clickable { onSeek(n) })
                            Text(n.label, Modifier.weight(1f).clickable { onSeek(n) }, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            TextButton(onClick = { onRemove(n) }) { Text("Retirer") }
                        }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = { onDone(text) }) { Text("Poser la note") } },
        dismissButton = { TextButton(onClick = { onDone(null) }) { Text("Annuler") } },
    )
}
