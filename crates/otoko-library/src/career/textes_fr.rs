//! Réserves de phrases du moteur de textes, reprises telles quelles de Studio Vault
//! (`composer_vault/data/textes_fr.py`, fichier généré : ne pas modifier à la main, sauf à garder
//! les clés, qui servent à ne pas réutiliser une phrase sur des publications rapprochées).

/// (clé, texte, champs requis).
pub type Frag = (&'static str, &'static str, &'static [&'static str]);

pub const TONS: &[(&str, &str)] = &[("mysterieux", "Mystérieux"), ("emotionnel", "Émotionnel"), ("technique", "Technique"), ("audacieux", "Audacieux"), ("intime", "Intime"), ("sobre", "Sobre")];

pub const ACCROCHES: &[(&str, &[Frag])] = &[
    ("mysterieux", &[
        ("m01", "Quelque part entre trois heures du matin et l'aube, {titre} a pris forme.", &[]),
        ("m02", "{titre}. La nuit lui appartient.", &[]),
        ("m03", "Certains sons ne naissent que dans le noir. {titre} en fait partie.", &[]),
        ("m04", "On ne sait jamais vraiment où mène {titre}. C'est justement pour ça qu'il faut y aller.", &[]),
        ("m05", "{titre} n'explique rien. Il s'écoute, porte fermée, lumière éteinte.", &[]),
        ("m06", "Il y a une pièce sans fenêtre dans ma tête. {titre} vient de là.", &[]),
        ("m07", "Une ambiance {amb_f}, une porte entrouverte : voici {titre}.", &["amb_f"]),
        ("m08", "{titre} avance à pas feutrés, puis ne vous lâche plus.", &[]),
        ("m09", "Ce qui se cache sous la surface de {titre}, je vous laisse le trouver.", &[]),
        ("m10", "Les signaux étaient là depuis longtemps. {titre} les rassemble.", &[]),
        ("m11", "{titre} : {bpm} pulsations par minute et beaucoup de choses non dites.", &["bpm"]),
        ("m12", "Un couloir sans fin, un battement régulier. {titre} commence ici.", &[]),
        ("m13", "Je l'ai écrit en {tonalite}, la couleur des choses qu'on garde pour soi. {titre}.", &["tonalite"]),
        ("m14", "Dernière pièce du puzzle {de_sortie} : {titre}.", &["de_sortie"]),
        ("m15", "{titre} s'est construit en silence, couche après couche.", &[]),
        ("m16", "Fermez les yeux trente secondes. {titre} fera le reste.", &[]),
    ]),
    ("emotionnel", &[
        ("e01", "J'ai mis beaucoup de moi dans {titre}. J'espère qu'il vous trouvera au bon moment.", &[]),
        ("e02", "{titre} est né d'un sentiment difficile à nommer. Je vous le confie tel quel.", &[]),
        ("e03", "Il y a des morceaux qu'on fait, et d'autres qui nous font. {titre} est de ceux-là.", &[]),
        ("e04", "{titre} parle de ce qui reste quand la lumière s'éteint.", &[]),
        ("e05", "Je n'avais pas les mots, alors j'ai fait {titre}.", &[]),
        ("e06", "{titre}, c'est une ambiance {amb_f} et un peu de tout ce que je n'ai pas su dire.", &["amb_f"]),
        ("e07", "Ce morceau m'a accompagné pendant des semaines. Aujourd'hui, {titre} est à vous.", &[]),
        ("e08", "Certains soirs ressemblent à {titre}. Celui-ci, je l'ai gardé en mémoire.", &[]),
        ("e09", "{titre} a commencé par une boucle jouée cent fois, jusqu'à ce qu'elle me réponde.", &[]),
        ("e10", "On a construit {titre} ensemble, {avec_collab}, et ça s'entend.", &["avec_collab"]),
        ("e11", "Il y a une part de {ville} dans {titre}. Ses nuits, ses lumières, ses silences.", &["ville"]),
        ("e12", "{titre} ouvre une parenthèse {amb_f}. Prenez le temps d'y entrer.", &["amb_f"]),
        ("e13", "J'ai failli ne jamais le sortir. Heureusement, j'ai changé d'avis : {titre}.", &[]),
        ("e14", "{titre} fait partie {de_sortie}, et c'est peut-être le plus personnel.", &["de_sortie"]),
        ("e15", "Si vous ne deviez écouter qu'un morceau ce soir, que ce soit {titre}.", &[]),
    ]),
    ("technique", &[
        ("t01", "{titre} : un morceau {genre}, {bpm} BPM, en {tonalite}.", &["genre", "bpm", "tonalite"]),
        ("t02", "Nouveau morceau {genre} : {titre}, calé à {bpm} BPM.", &["genre", "bpm"]),
        ("t03", "{titre}, {bpm} BPM. Pensé pour l'espace, le poids et le mouvement.", &["bpm"]),
        ("t04", "Sur {titre}, tout part d'une ligne de basse en {tonalite}.", &["tonalite"]),
        ("t05", "{titre} : {duree} de {genre} construits autour d'une ambiance {amb_f}.", &["duree", "genre", "amb_f"]),
        ("t06", "Batterie sèche, basse ronde, espace large : voici {titre}.", &[]),
        ("t07", "{titre} a été pensé pour les grands systèmes son comme pour le casque.", &[]),
        ("t08", "Un travail de textures plus que de mélodies : {titre}.", &[]),
        ("t09", "{titre} tient sur une seule idée, déclinée sur {duree}.", &["duree"]),
        ("t10", "Nouveau titre {de_sortie} : {titre}, {bpm} BPM.", &["de_sortie", "bpm"]),
        ("t11", "{titre} en {tonalite} : une progression lente, des couches qui s'empilent.", &["tonalite"]),
        ("t12", "Production, mixage et arrangement maison pour {titre}.", &[]),
        ("t13", "Pour les curieux : {titre} tourne à {bpm} BPM en {tonalite}.", &["bpm", "tonalite"]),
        ("t14", "{titre} : un morceau {genre} sans compromis.", &["genre"]),
    ]),
    ("audacieux", &[
        ("a01", "{titre} ne demande pas la permission.", &[]),
        ("a02", "Montez le son. {titre} n'a pas été fait pour être écouté bas.", &[]),
        ("a03", "{titre} arrive, et il ne fait pas dans la demi-mesure.", &[]),
        ("a04", "{bpm} BPM, aucune pause. {titre}.", &["bpm"]),
        ("a05", "On a cassé quelques règles pour {titre}. Aucun regret.", &[]),
        ("a06", "{titre} : un morceau {genre} qui va droit au but.", &["genre"]),
        ("a07", "Pas d'introduction polie. {titre} entre directement dans le vif.", &[]),
        ("a08", "{titre} a été fait pour les murs qui tremblent.", &[]),
        ("a09", "Il fallait que ça sorte. {titre} est là.", &[]),
        ("a10", "{titre}, {avec_collab}. Préparez-vous.", &["avec_collab"]),
        ("a11", "Une ambiance {amb_f}, une basse qui ne lâche rien : {titre}.", &["amb_f"]),
        ("a12", "{titre} ouvre {sortie}, et il ouvre fort.", &["sortie"]),
        ("a13", "Si {titre} ne vous fait pas bouger, vérifiez vos enceintes.", &[]),
        ("a14", "Le genre de morceau qu'on remet dès qu'il se termine : {titre}.", &[]),
    ]),
    ("intime", &[
        ("i01", "{titre}, à écouter seul, au casque, tard.", &[]),
        ("i02", "Un morceau calme pour une fois : {titre}.", &[]),
        ("i03", "{titre} est né un dimanche soir, sans prévenir.", &[]),
        ("i04", "Je vous partage {titre} comme on partage un secret.", &[]),
        ("i05", "{titre} : peu d'éléments, beaucoup d'espace.", &[]),
        ("i06", "Il y a une ambiance {amb_f} dans {titre} que je n'arrive pas à expliquer.", &["amb_f"]),
        ("i07", "{titre} a la couleur de {ville} quand tout le monde dort.", &["ville"]),
        ("i08", "J'ai gardé {titre} pour moi pendant un moment. Il est temps.", &[]),
        ("i09", "Rien d'autre qu'une boucle, un souffle, et {titre}.", &[]),
        ("i10", "{titre} se murmure plus qu'il ne se joue.", &[]),
        ("i11", "Une pause de {duree}. {titre}.", &["duree"]),
        ("i12", "{titre}, en {tonalite}, pour les heures où on pense trop.", &["tonalite"]),
        ("i13", "Un des titres les plus doux {de_sortie} : {titre}.", &["de_sortie"]),
    ]),
    ("sobre", &[
        ("s01", "{titre} est disponible.", &[]),
        ("s02", "Nouveau morceau : {titre}.", &[]),
        ("s03", "{titre}, maintenant en écoute.", &[]),
        ("s04", "{titre} : un morceau {genre}.", &["genre"]),
        ("s05", "{titre}, extrait {de_sortie}.", &["de_sortie"]),
        ("s06", "{titre}, {avec_collab}.", &["avec_collab"]),
        ("s07", "Voici {titre}.", &[]),
        ("s08", "{titre} est en ligne.", &[]),
    ]),
];

pub const CORPS: &[(&str, &[Frag])] = &[
    ("mysterieux", &[
        ("mc01", "Baissez les lumières, oubliez l'heure.", &[]),
        ("mc02", "Il faut le prendre en entier, pas en extrait.", &[]),
        ("mc03", "Une plongée {amb_f} qu'on ne contrôle pas tout à fait.", &["amb_f"]),
        ("mc04", "Chaque écoute révèle un détail qui avait échappé à la précédente.", &[]),
        ("mc05", "Le genre de morceau qui reste dans la pièce après la dernière note.", &[]),
        ("mc06", "Une ambiance {amb_f} et {amb2_f}, comme un rêve dont on se souvient à moitié.", &["amb_f", "amb2_f"]),
        ("mc07", "Laissez-le tourner jusqu'au bout : la fin raconte autre chose.", &[]),
        ("mc08", "Rien n'est laissé au hasard, même ce qui semble flotter.", &[]),
        ("mc09", "Une boucle qui tourne comme une pensée qu'on n'arrive pas à chasser.", &[]),
    ]),
    ("emotionnel", &[
        ("ec01", "Je ne vais pas vous dire quoi ressentir. Écoutez, simplement.", &[]),
        ("ec02", "C'est le genre de morceau qui prend tout son sens au bon moment.", &[]),
        ("ec03", "Il y a une ambiance {amb_f} qui traverse tout le morceau, du premier au dernier souffle.", &["amb_f"]),
        ("ec04", "Merci à celles et ceux qui écoutent jusqu'au bout. Ça compte plus que vous ne le pensez.", &[]),
        ("ec05", "Je l'ai réécouté des dizaines de fois avant de le lâcher. Il est prêt.", &[]),
        ("ec06", "Un morceau à écouter en marchant, sans destination précise.", &[]),
        ("ec07", "Si ce morceau vous parle, dites-le-moi. J'aime savoir où il atterrit.", &[]),
        ("ec08", "Quelque chose entre la nostalgie et l'envie d'avancer.", &[]),
        ("ec09", "Une ambiance {amb_f} et {amb2_f}, à l'image de ces derniers mois.", &["amb_f", "amb2_f"]),
    ]),
    ("technique", &[
        ("tc01", "Kick sec, basse ronde, beaucoup d'air dans les aigus.", &[]),
        ("tc02", "Tout est construit pour que la tension monte sans jamais exploser trop tôt.", &[]),
        ("tc03", "Le mixage laisse de la place à chaque élément : rien ne se marche dessus.", &[]),
        ("tc04", "Une structure longue, pensée pour le mix comme pour l'écoute.", &[]),
        ("tc05", "Les textures ont été enregistrées puis retravaillées jusqu'à devenir méconnaissables.", &[]),
        ("tc06", "Une ambiance {amb_f}, obtenue avec très peu d'éléments.", &["amb_f"]),
        ("tc07", "Le tempo de {bpm} BPM laisse respirer le groove.", &["bpm"]),
        ("tc08", "La tonalité de {tonalite} donne cette couleur particulière au morceau.", &["tonalite"]),
        ("tc09", "Pensé pour les DJ : intro et outro propres, faciles à mixer.", &[]),
    ]),
    ("audacieux", &[
        ("ac01", "Aucune concession, aucun remplissage.", &[]),
        ("ac02", "Écoutez-le fort, ou ne l'écoutez pas.", &[]),
        ("ac03", "Une ambiance {amb_f} du début à la fin, sans temps mort.", &["amb_f"]),
        ("ac04", "Fait pour la nuit, les clubs et les trajets trop longs.", &[]),
        ("ac05", "La basse a été pensée pour se sentir autant que pour s'entendre.", &[]),
        ("ac06", "Testé sur de gros systèmes son. Validé.", &[]),
        ("ac07", "Ça cogne, ça respire, puis ça cogne encore.", &[]),
        ("ac08", "Partagez-le à quelqu'un qui a besoin d'un coup de fouet.", &[]),
    ]),
    ("intime", &[
        ("ic01", "Peu de mots aujourd'hui. Le morceau parle mieux que moi.", &[]),
        ("ic02", "Un moment {amb_m}, rien de plus, rien de moins.", &["amb_m"]),
        ("ic03", "À écouter au casque pour ne rien perdre des détails.", &[]),
        ("ic04", "Prenez-le comme une pause dans la journée.", &[]),
        ("ic05", "Il a été fait la nuit, il s'écoute la nuit.", &[]),
        ("ic06", "Une ambiance {amb_f} et {amb2_f}, toute en retenue.", &["amb_f", "amb2_f"]),
        ("ic07", "Pas de grand discours. Juste un morceau qui me ressemble.", &[]),
        ("ic08", "Si vous l'écoutez sous la pluie, racontez-moi.", &[]),
    ]),
    ("sobre", &[
        ("sc01", "Bonne écoute.", &[]),
        ("sc02", "Ambiance {amb_f}.", &["amb_f"]),
        ("sc03", "Durée : {duree}.", &["duree"]),
        ("sc04", "{bpm} BPM, {tonalite}.", &["bpm", "tonalite"]),
    ]),
];

pub const APPELS_SOUNDCLOUD: &[Frag] = &[
    ("cs01", "Un repost aide énormément si le morceau vous plaît.", &[]),
    ("cs02", "Laissez un commentaire au moment qui vous parle le plus.", &[]),
    ("cs03", "Abonnez-vous pour ne pas rater la suite.", &[]),
    ("cs04", "Écoutez-le en entier, la fin vaut le détour.", &[]),
    ("cs05", "Ajoutez-le à vos playlists de nuit.", &[]),
    ("cs06", "Dites-moi en commentaire à quel moment il vous a eu.", &[]),
    ("cs07", "Partagez-le à quelqu'un qui devrait l'entendre.", &[]),
    ("cs08", "Merci pour chaque écoute, chaque like, chaque repost.", &[]),
    ("cs09", "Le reste {de_sortie} est déjà en ligne sur mon profil.", &["de_sortie"]),
    ("cs10", "D'autres morceaux arrivent bientôt. Restez dans le coin.", &[]),
    ("cs11", "Un petit like, et l'algorithme le montrera à d'autres oreilles.", &[]),
    ("cs12", "Je lis tous les commentaires.", &[]),
];

pub const APPELS_INSTAGRAM: &[Frag] = &[
    ("ci01", "Lien en bio.", &[]),
    ("ci02", "En écoute sur SoundCloud, lien en bio.", &[]),
    ("ci03", "Le morceau complet est sur SoundCloud, lien en bio.", &[]),
    ("ci04", "Volume à fond. Lien en bio.", &[]),
    ("ci05", "Dites-moi en commentaire ce que ça vous évoque.", &[]),
    ("ci06", "Partagez-le en story si vous l'aimez.", &[]),
    ("ci07", "Enregistrez ce post pour l'écouter ce soir.", &[]),
    ("ci08", "Identifiez quelqu'un qui doit l'entendre.", &[]),
    ("ci09", "Dispo maintenant sur SoundCloud.", &[]),
    ("ci10", "Casque recommandé. Lien en bio.", &[]),
    ("ci11", "Écoute complète en bio, à faire tard le soir.", &[]),
    ("ci12", "Un repost, un partage, un commentaire : tout aide.", &[]),
];

pub const ACCROCHES_STORY: &[Frag] = &[
    ("st01", "{titre} est en ligne.", &[]),
    ("st02", "Nouveau son : {titre}.", &[]),
    ("st03", "{titre}, maintenant sur SoundCloud.", &[]),
    ("st04", "Montez le son : {titre}.", &[]),
    ("st05", "{titre} vient de sortir.", &[]),
    ("st06", "Ce soir, c'est {titre}.", &[]),
    ("st07", "{titre}, {bpm} BPM.", &["bpm"]),
    ("st08", "Ambiance {amb_f} : {titre}.", &["amb_f"]),
    ("st09", "{titre}, extrait {de_sortie}.", &["de_sortie"]),
    ("st10", "{titre}, {avec_collab}.", &["avec_collab"]),
    ("st11", "Casque obligatoire : {titre}.", &[]),
    ("st12", "Le nouveau morceau est là : {titre}.", &[]),
];

pub const APPELS_STORY: &[Frag] = &[
    ("sa01", "Lien en bio.", &[]),
    ("sa02", "Glissez pour écouter.", &[]),
    ("sa03", "Écoute complète sur SoundCloud.", &[]),
    ("sa04", "Dispo maintenant.", &[]),
    ("sa05", "Lien juste ici.", &[]),
];

pub const CREDITS: &[Frag] = &[
    ("cr01", "Composé et produit par {artiste}.", &[]),
    ("cr02", "Écrit, produit et mixé par {artiste}.", &[]),
    ("cr03", "Une production {artiste}.", &[]),
];

pub const CREDITS_COLLAB: &[Frag] = &[
    ("cc01", "Composé et produit par {artiste}, {avec_collab}.", &["avec_collab"]),
    ("cc02", "Une collaboration entre {artiste} et {collab}.", &["collab"]),
];

/// famille → (mots-clés de détection, libellé par défaut), dans l'ordre de recherche.
pub const FAMILLES: &[(&str, &[&str], &str)] = &[
    ("techno_melodique", &["melodic techno", "techno mélodique", "techno melodique"], "techno mélodique"),
    ("techno", &["techno", "industrial", "industriel", "hard techno", "acid"], "techno"),
    ("afro_house", &["afro house", "afro"], "afro house"),
    ("house", &["house", "garage", "disco", "nu disco"], "house"),
    ("trance", &["trance", "psytrance", "psy"], "trance"),
    ("drum_and_bass", &["drum", "dnb", "jungle", "neuro", "breakbeat", "breaks"], "drum & bass"),
    ("downtempo", &["downtempo", "trip hop", "trip-hop", "chill", "lo-fi", "lofi"], "downtempo"),
    ("ambient", &["ambient", "drone", "atmospheric", "atmosphérique", "new age"], "ambient"),
    ("cinematique", &["cinematic", "cinématique", "cinematique", "orchestral", "score", "bande originale", "neoclassical", "néoclassique"], "cinématique"),
    ("synthwave", &["synthwave", "retrowave", "darkwave", "outrun"], "synthwave"),
    ("hiphop", &["hip hop", "hip-hop", "rap", "trap", "boom bap", "drill"], "hip-hop"),
    ("rock", &["rock", "punk", "grunge", "metal", "indie"], "rock"),
    ("pop", &["pop", "synthpop", "hyperpop", "chanson", "variété"], "pop"),
    ("experimental", &["experimental", "expérimental", "glitch", "idm", "noise"], "expérimental"),
    ("electronique", &["electro", "électro", "electronic", "électronique", "edm", "dubstep", "bass"], "électronique"),
];

pub const HASHTAGS_LARGES: &[&str] = &["#musique", "#musiqueelectronique", "#nouveauson", "#producteur", "#compositeur", "#soundcloud", "#musiquefrancaise", "#homestudio"];

pub const HASHTAGS_FAMILLE: &[(&str, &[&str])] = &[
    ("techno_melodique", &["#technomelodique", "#melodictechno", "#techno", "#technofrance", "#afterhours", "#underground"]),
    ("techno", &["#techno", "#technofrance", "#technomusic", "#rave", "#underground", "#clubbing", "#technohypnotique"]),
    ("afro_house", &["#afrohouse", "#house", "#housemusic", "#deephouse", "#groove", "#dancefloor"]),
    ("house", &["#house", "#housemusic", "#deephouse", "#groove", "#dancefloor", "#housefrancaise"]),
    ("trance", &["#trance", "#trancefamily", "#psytrance", "#progressive", "#rave", "#euphorie"]),
    ("drum_and_bass", &["#drumandbass", "#dnb", "#jungle", "#bassmusic", "#neurofunk", "#dnbfrance"]),
    ("downtempo", &["#downtempo", "#triphop", "#chill", "#lofi", "#musiquedetente", "#nightvibes"]),
    ("ambient", &["#ambient", "#musiqueambient", "#paysagesonore", "#atmosphere", "#contemplation", "#drone"]),
    ("cinematique", &["#musiquedefilm", "#cinematique", "#bandeoriginale", "#orchestral", "#compositeurdefilm", "#musiqueinstrumentale"]),
    ("synthwave", &["#synthwave", "#retrowave", "#darkwave", "#annees80", "#synthes", "#outrun"]),
    ("hiphop", &["#hiphop", "#rapfrancais", "#beatmaker", "#instru", "#prod", "#boombap"]),
    ("rock", &["#rock", "#rockfrancais", "#guitare", "#indie", "#groupe", "#rockalternatif"]),
    ("pop", &["#pop", "#popfrancaise", "#chanson", "#nouveautemusicale", "#chanteur", "#melodie"]),
    ("experimental", &["#musiqueexperimentale", "#experimental", "#sounddesign", "#avantgarde", "#idm", "#bruitiste"]),
    ("electronique", &["#electro", "#musiqueelectronique", "#electronique", "#synthe", "#sounddesign", "#electrofrancaise"]),
];

pub const HASHTAGS_AMBIANCE: &[(&str, &str)] = &[
    ("sombre", "#musiquesombre"),
    ("mélancolique", "#melancolie"),
    ("nocturne", "#musiquedenuit"),
    ("hypnotique", "#hypnotique"),
    ("tendu", "#tension"),
    ("brut", "#brut"),
    ("intense", "#intense"),
    ("puissant", "#puissance"),
    ("euphorique", "#euphorie"),
    ("solaire", "#solaire"),
    ("lumineux", "#lumineux"),
    ("aérien", "#aerien"),
    ("contemplatif", "#contemplation"),
    ("planant", "#planant"),
    ("feutré", "#douceur"),
    ("intime", "#intime"),
    ("fiévreux", "#fievre"),
    ("dansant", "#dansant"),
    ("rêveur", "#reverie"),
    ("mystérieux", "#mystere"),
    ("cinématique", "#cinematique"),
    ("organique", "#organique"),
    ("énergique", "#energie"),
    ("doux", "#douceur"),
    ("orageux", "#orage"),
    ("glacé", "#froid"),
    ("chaleureux", "#chaleur"),
    ("industriel", "#industriel"),
    ("spirituel", "#spirituel"),
    ("nostalgique", "#nostalgie"),
];

pub const HASHTAGS_BANNIS: &[&str] = &["#ai", "#f4f", "#followforfollow", "#ia", "#l4l", "#like4like", "#sub4sub", "#suno"];

pub const MOTS_TOUJOURS_INTERDITS: &[&str] = &["suno", "intelligence artificielle", "généré par", "généré avec", " ia ", "ai-generated"];

pub const GENRE_SOUNDCLOUD: &[(&str, &str)] = &[
    ("techno_melodique", "Techno"),
    ("techno", "Techno"),
    ("afro_house", "House"),
    ("house", "House"),
    ("trance", "Trance"),
    ("drum_and_bass", "Drum & Bass"),
    ("downtempo", "Downtempo"),
    ("ambient", "Ambient"),
    ("cinematique", "Classical"),
    ("synthwave", "Electronic"),
    ("hiphop", "Hip-hop & Rap"),
    ("rock", "Rock"),
    ("pop", "Pop"),
    ("experimental", "Electronic"),
    ("electronique", "Electronic"),
];

pub const TAGS_SOUNDCLOUD: &[(&str, &[&str])] = &[
    ("techno_melodique", &["techno mélodique", "melodic techno", "techno", "afterhours"]),
    ("techno", &["techno", "techno française", "underground", "rave"]),
    ("afro_house", &["afro house", "house", "groove"]),
    ("house", &["house", "deep house", "groove"]),
    ("trance", &["trance", "progressive", "psytrance"]),
    ("drum_and_bass", &["drum and bass", "dnb", "jungle"]),
    ("downtempo", &["downtempo", "trip hop", "chill"]),
    ("ambient", &["ambient", "paysage sonore", "atmosphérique"]),
    ("cinematique", &["musique de film", "cinématique", "orchestral"]),
    ("synthwave", &["synthwave", "retrowave", "darkwave"]),
    ("hiphop", &["hip-hop", "instru", "beat"]),
    ("rock", &["rock", "indie", "alternatif"]),
    ("pop", &["pop", "pop française", "chanson"]),
    ("experimental", &["expérimental", "sound design", "idm"]),
    ("electronique", &["électronique", "électro", "musique électronique"]),
];

pub const LIMITE_SOUNDCLOUD: usize = 4000;
pub const LIMITE_INSTAGRAM: usize = 2200;
pub const LIMITE_STORY: usize = 160;

pub const NOTES_FR: &[(&str, &str)] = &[
    ("C", "do"),
    ("C#", "do dièse"),
    ("D", "ré"),
    ("D#", "mi bémol"),
    ("E", "mi"),
    ("F", "fa"),
    ("F#", "fa dièse"),
    ("G", "sol"),
    ("G#", "la bémol"),
    ("A", "la"),
    ("A#", "si bémol"),
    ("B", "si"),
    ("Db", "ré bémol"),
    ("Eb", "mi bémol"),
    ("Gb", "sol bémol"),
    ("Ab", "la bémol"),
    ("Bb", "si bémol"),
];

pub const TYPES_SORTIE: &[(&str, &str, &str)] = &[
    ("remixes", "les remixes", "des remixes"),
    ("single", "le single", "du single"),
    ("ep", "l'EP", "de l'EP"),
    ("album", "l'album", "de l'album"),
    ("compilation", "la compilation", "de la compilation"),
    ("mixtape", "la mixtape", "de la mixtape"),
];

pub const ACCROCHES_NATURE: &[(&str, &[Frag])] = &[
    ("remix", &[
        ("nr01", "Ma relecture de {orig} : {titre}.", &["orig"]),
        ("nr02", "J'ai repris {orig} pour l'emmener ailleurs. Voici {titre}.", &["orig"]),
        ("nr03", "Un remix que j'avais en tête depuis longtemps : {titre}.", &[]),
        ("nr04", "{titre} — {orig}, passé à ma sauce.", &["orig"]),
        ("nr05", "Quand on me confie un morceau, j'essaie d'en garder l'âme. {titre}.", &[]),
        ("nr06", "Nouveau remix : {titre}, dans une ambiance {amb_f}.", &["amb_f"]),
    ]),
    ("edit", &[
        ("ne01", "Un edit maison de {orig}, taillé pour le club : {titre}.", &["orig"]),
        ("ne02", "Petit edit de {orig} que je joue dans mes sets.", &["orig"]),
        ("ne03", "{titre} : mon edit, à écouter fort.", &[]),
    ]),
    ("mashup", &[
        ("nm01", "Deux univers qui n'auraient jamais dû se croiser : {titre}.", &[]),
        ("nm02", "Un mashup qui me trottait dans la tête : {titre}.", &[]),
        ("nm03", "{titre} : quand des morceaux que j'aime se rencontrent.", &[]),
    ]),
    ("set", &[
        ("ns01", "Nouveau mix : {duree} de son, {nb_titres} titres.", &["duree", "nb_titres"]),
        ("ns02", "{titre} : un set à écouter d'une traite.", &[]),
        ("ns03", "J'ai enregistré ce mix pour vous : {titre}.", &[]),
        ("ns04", "{duree} de voyage, une ambiance {amb_f} du début à la fin.", &["duree", "amb_f"]),
        ("ns05", "Le mix est en ligne : {titre}. Liste des titres plus bas.", &[]),
    ]),
    ("prod", &[
        ("np01", "Une fierté d'avoir produit {titre} pour {client}.", &["client"]),
        ("np02", "Une prod signée pour {client} : {titre}.", &["client"]),
        ("np03", "{titre}, produit en studio avec {client}.", &["client"]),
    ]),
];

pub const HASHTAGS_NATURE: &[(&str, &[&str])] = &[
    ("remix", &["#remix", "#remixe"]),
    ("edit", &["#edit", "#bootleg"]),
    ("mashup", &["#mashup"]),
    ("set", &["#djset", "#mix", "#dj"]),
    ("prod", &["#prod", "#beatmaker"]),
];

pub const TAGS_SC_NATURE: &[(&str, &[&str])] = &[
    ("remix", &["remix"]),
    ("edit", &["edit", "bootleg"]),
    ("mashup", &["mashup"]),
    ("set", &["dj set", "mix"]),
    ("prod", &["prod"]),
];
