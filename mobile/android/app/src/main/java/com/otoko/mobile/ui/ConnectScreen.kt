package com.otoko.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.otoko.mobile.MainViewModel
import kotlinx.coroutines.launch

private val ErrorText = Color(0xFFFF8A75)

/** Premier lancement : l'appli ne connaît aucun PC, chacun relie le sien. */
@Composable
fun ConnectScreen(vm: MainViewModel, address: String, canCancel: Boolean) {
    Box(Modifier.fillMaxSize().background(Palette.Black).statusBarsPadding().navigationBarsPadding().imePadding()) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 40.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text("Otoko Vault", fontFamily = Display, fontWeight = FontWeight.Bold, fontSize = 38.sp, color = Palette.White)
            Text("Ta musique, depuis ton PC.", fontFamily = Body, fontSize = 18.sp, color = Palette.White)
            Text(
                if (address.isNotEmpty()) "Voici l'adresse de ton PC. Tailscale doit être connecté sur ce téléphone."
                else "Sur ton PC, ouvre Otoko Studio Vault Pro, page Mobile et partage, et scanne le second code : " +
                    "« Ouvrir l'app » remplit l'adresse pour toi. Tu peux aussi la taper ici.",
                fontFamily = Body, fontSize = 15.sp, color = Palette.Muted)
            ConnectForm(vm, address)
            if (canCancel) Text(
                "Annuler", fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, color = Palette.White,
                modifier = Modifier.clickable { vm.dropSuggestion() }.padding(vertical = 8.dp))
        }
    }
}

/** Le champ d'adresse et son bouton. */
@Composable
fun ConnectForm(vm: MainViewModel, current: String, onDone: () -> Unit = {}) {
    val scope = rememberCoroutineScope()
    var address by remember(current) { mutableStateOf(current) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val submit = {
        if (!busy) {
            busy = true
            problem = null
            scope.launch {
                problem = vm.connectTo(address)
                busy = false
                if (problem == null) onDone()
            }
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(
            Modifier.fillMaxWidth().height(52.dp).clip(RoundedCornerShape(14.dp)).background(Palette.Surface).padding(horizontal = 16.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (address.isEmpty()) Text("mon-pc.xxxx.ts.net", fontFamily = Body, fontSize = 16.sp, color = Palette.Muted)
            BasicTextField(
                address, { address = it; problem = null }, singleLine = true, cursorBrush = SolidColor(Palette.White),
                textStyle = TextStyle(color = Palette.White, fontFamily = Body, fontSize = 16.sp),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go, autoCorrectEnabled = false),
                keyboardActions = KeyboardActions(onGo = { submit() }),
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Adresse de ton PC" })
        }
        Box(
            Modifier.fillMaxWidth().height(50.dp).clip(RoundedCornerShape(25.dp)).background(Palette.White)
                .clickable(enabled = !busy) { submit() },
            contentAlignment = Alignment.Center,
        ) {
            Text(if (busy) "Connexion…" else "Se connecter", fontFamily = Body, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, color = Palette.Black)
        }
        problem?.let { Text(it, fontFamily = Body, fontSize = 14.sp, color = ErrorText) }
    }
}
