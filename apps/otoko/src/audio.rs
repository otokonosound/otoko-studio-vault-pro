//! La sortie d'écoute du lecteur. Elle ne s'ouvre que le temps d'écouter : au repos, le logiciel ne tient aucune carte
//! son et ne fait tourner aucun fil audio. C'est la sortie de Windows, partagée avec les autres logiciels : FL Studio et
//! les autres gardent leur pilote.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::Consumer;

/// Taille de bloc la plus grande qu'on prépare (les cartes en demandent bien moins).
const MAX_BLOCK: usize = 8192;

/// Ce que le fil audio partage avec l'interface.
#[derive(Default)]
struct Partage {
    /// Le morceau, décodé en avance dans un anneau d'échantillons stéréo entrelacés.
    source: Mutex<Option<Consumer<f32>>>,
    /// Images déjà jouées depuis le début du morceau.
    position: AtomicU64,
    /// L'anneau s'est vidé : le décodage est en retard, ou le morceau est fini.
    a_sec: AtomicBool,
    pause: AtomicBool,
    /// Gain (bits d'un f32).
    gain: AtomicU32,
}

impl Partage {
    /// Remplit un bloc. Sans attente : un verrou pris (un autre morceau qui arrive) rend un bloc de silence.
    fn rendre(&self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        if self.pause.load(Ordering::Relaxed) {
            return;
        }
        let Ok(mut source) = self.source.try_lock() else { return };
        let Some(ring) = source.as_mut() else { return };
        let frames = left.len().min(right.len());
        let dispo = (ring.slots() / 2).min(frames);
        let gain = f32::from_bits(self.gain.load(Ordering::Relaxed));
        if let Ok(chunk) = ring.read_chunk(dispo * 2) {
            let (a, b) = chunk.as_slices();
            let mut it = a.iter().chain(b.iter());
            for i in 0..dispo {
                left[i] = it.next().copied().unwrap_or(0.0) * gain;
                right[i] = it.next().copied().unwrap_or(0.0) * gain;
            }
            chunk.commit_all();
        }
        self.position.fetch_add(dispo as u64, Ordering::Relaxed);
        self.a_sec.store(dispo < frames, Ordering::Relaxed);
    }
}

/// Une carte son ouverte.
struct Sortie {
    _windows: Option<cpal::Stream>,
    /// `--silent` : pas de carte, un fil consomme le morceau au rythme réel ; ce drapeau l'arrête.
    muette: Option<Arc<AtomicBool>>,
    description: String,
    rate: f64,
}

impl Drop for Sortie {
    fn drop(&mut self) {
        if let Some(stop) = &self.muette {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// La sortie d'écoute : la carte, tant qu'elle est ouverte.
pub struct Ecoute {
    partage: Arc<Partage>,
    sortie: Option<Sortie>,
    /// `--silent` : la lecture avance sans ouvrir de carte (l'autotest ne dérange ni FL Studio ni les enceintes).
    muet: bool,
}

impl Ecoute {
    pub fn new(muet: bool) -> Ecoute {
        let partage = Arc::new(Partage::default());
        partage.gain.store(1f32.to_bits(), Ordering::Relaxed);
        Ecoute { partage, sortie: None, muet }
    }

    /// Ouvre la carte si elle ne l'est pas ; rend sa fréquence.
    pub fn ouvrir(&mut self) -> Result<f64, String> {
        if let Some(s) = &self.sortie {
            return Ok(s.rate);
        }
        let s = if self.muet { open_muette(&self.partage) } else { open_windows(&self.partage)? };
        let rate = s.rate;
        self.sortie = Some(s);
        Ok(rate)
    }

    /// Rend la carte aux autres logiciels.
    pub fn fermer(&mut self) {
        self.sortie = None;
        self.couper();
    }

    pub fn ouverte(&self) -> bool {
        self.sortie.is_some()
    }

    pub fn rate(&self) -> f64 {
        self.sortie.as_ref().map(|s| s.rate).unwrap_or(48000.0)
    }

    /// La carte ouverte, en toutes lettres.
    pub fn description(&self) -> Option<&str> {
        self.sortie.as_ref().map(|s| s.description.as_str())
    }

    /// Fait jouer un anneau de morceau, à partir de l'image `start`.
    pub fn lancer(&self, ring: Consumer<f32>, start: u64) {
        let ancien = {
            let mut source = self.partage.source.lock().unwrap_or_else(|e| e.into_inner());
            self.partage.position.store(start, Ordering::Relaxed);
            self.partage.a_sec.store(false, Ordering::Relaxed);
            self.partage.pause.store(false, Ordering::Relaxed);
            source.replace(ring)
        };
        // L'ancien anneau se libère ici, pas dans le fil audio.
        drop(ancien);
    }

    pub fn couper(&self) {
        let ancien = self.partage.source.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(ancien);
    }

    pub fn pause(&self, on: bool) {
        self.partage.pause.store(on, Ordering::Relaxed);
    }

    pub fn gain(&self, g: f32) {
        self.partage.gain.store(if self.muet { 0f32 } else { g }.to_bits(), Ordering::Relaxed);
    }

    /// Images jouées depuis le début du morceau.
    pub fn position(&self) -> u64 {
        self.partage.position.load(Ordering::Relaxed)
    }

    pub fn a_sec(&self) -> bool {
        self.partage.a_sec.load(Ordering::Relaxed)
    }
}

/// Inscrit le fil courant dans le planificateur multimédia de Windows (« Pro Audio ») : pas de coupure quand le PC
/// travaille à côté (analyse, mastering).
fn fil_prioritaire() {
    #[link(name = "avrt")]
    unsafe extern "system" {
        fn AvSetMmThreadCharacteristicsW(task: *const u16, index: *mut u32) -> isize;
    }
    let task: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
    let mut index = 0u32;
    unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
}

fn open_muette(partage: &Arc<Partage>) -> Sortie {
    const RATE: f64 = 48000.0;
    let stop = Arc::new(AtomicBool::new(false));
    let (partage, arret) = (partage.clone(), stop.clone());
    let _ = std::thread::Builder::new().name("sortie-muette".into()).spawn(move || {
        let (mut left, mut right) = (vec![0.0f32; MAX_BLOCK], vec![0.0f32; MAX_BLOCK]);
        let mut last = std::time::Instant::now();
        while !arret.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let frames = ((last.elapsed().as_secs_f64() * RATE) as usize).min(MAX_BLOCK);
            last = std::time::Instant::now();
            partage.rendre(&mut left[..frames], &mut right[..frames]);
        }
    });
    Sortie { rate: RATE, _windows: None, muette: Some(stop), description: "sortie muette".into() }
}

fn open_windows(partage: &Arc<Partage>) -> Result<Sortie, String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("aucune sortie audio sur ce PC")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!("format de sortie non pris en charge : {:?}", config.sample_format()));
    }
    let rate = f64::from(config.sample_rate().0);
    let channels = usize::from(config.channels()).max(1);
    let partage = partage.clone();
    let (mut left, mut right) = (vec![0.0f32; MAX_BLOCK], vec![0.0f32; MAX_BLOCK]);
    let mut premier = true;
    let stream = device
        .build_output_stream(
            &config.config(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if premier {
                    fil_prioritaire();
                    premier = false;
                }
                for chunk in data.chunks_mut(MAX_BLOCK * channels) {
                    let frames = chunk.len() / channels;
                    partage.rendre(&mut left[..frames], &mut right[..frames]);
                    for f in 0..frames {
                        let o = &mut chunk[f * channels..(f + 1) * channels];
                        o[0] = left[f];
                        if channels > 1 {
                            o[1] = right[f];
                        }
                        o.iter_mut().skip(2).for_each(|x| *x = 0.0);
                    }
                }
            },
            |e| eprintln!("sortie audio : {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    #[allow(deprecated)]
    let name = device.name().unwrap_or_else(|_| "sortie par défaut".into());
    Ok(Sortie { rate, _windows: Some(stream), muette: None, description: format!("Windows · {name} · {} kHz", rate / 1000.0) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_bloc_rend_le_morceau_au_gain_voulu_et_dit_quand_c_est_fini() {
        let p = Partage::default();
        p.gain.store(0.5f32.to_bits(), Ordering::Relaxed);
        let (mut prod, cons) = rtrb::RingBuffer::new(16);
        for v in [1.0, -1.0, 0.5, -0.5] {
            prod.push(v).unwrap();
        }
        *p.source.lock().unwrap() = Some(cons);
        let (mut l, mut r) = ([9.0f32; 3], [9.0f32; 3]);
        p.rendre(&mut l, &mut r);
        assert_eq!(l, [0.5, 0.25, 0.0]);
        assert_eq!(r, [-0.5, -0.25, 0.0]);
        assert_eq!(p.position.load(Ordering::Relaxed), 2);
        assert!(p.a_sec.load(Ordering::Relaxed), "l'anneau est vide");
        p.pause.store(true, Ordering::Relaxed);
        p.rendre(&mut l, &mut r);
        assert_eq!(l, [0.0; 3]);
    }
}
