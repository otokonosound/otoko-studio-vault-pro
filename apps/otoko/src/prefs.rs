//! Réglages : la sortie d'écoute.

use crate::AppWindow;
use crate::state::State;

/// Remplit la partie sortie d'écoute des Réglages.
pub fn fill(app: &AppWindow, st: &State) {
    app.set_audio_text(
        match st.ecoute.description() {
            Some(d) => format!("En lecture sur : {d}."),
            None => "Le son sort par la sortie de Windows. La carte son ne s'ouvre que pendant l'écoute : le reste du temps, elle est libre pour tes autres logiciels.".to_string(),
        }
        .into(),
    );
}
