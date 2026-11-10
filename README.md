# Otoko Studio Vault Pro

**Ta musique rangée, retrouvée et écoutée partout.** Le logiciel des producteurs qui ont trop de morceaux et plus le temps de les chercher.

Tu pointes ton dossier de musique. Il reconnaît et analyse tes morceaux, sans jamais les déplacer ni les modifier. Ensuite tu les retrouves tout de suite, tu prépares tes sorties, et tu les écoutes sur ton téléphone ou dans ta voiture.

Logiciel libre et gratuit, en français, pour Windows, avec une app Android.

**[Regarder la vidéo de présentation](https://github.com/otokonosound/otoko-studio-vault-pro/releases/latest/download/Otoko-Studio-Vault-Pro-presentation.mp4)** (2 min 43, tout ce qu'il sait faire).

## Ce qu'il fait

- **Bibliothèque** : tempo, tonalité, loudness, énergie et ambiance détectés tout seuls ; recherche du genre `bpm:120-128 key:Am` ; tableau ou cartes ; vues gardées ; palette Ctrl+K pour tout trouver.
- **Écoute** : waveform, marqueurs, boucle A-B, notes, file d'attente, comparaison de deux morceaux à volume égal, « Volume égal » pour toute l'écoute, tri des idées au clavier, morceaux compatibles et roue de Camelot.
- **Outils** : renommer, réorganiser, trouver les doublons, compléter genres et artistes, éditer en lot, exporter en CSV. Ctrl+Z annule la dernière modification, de fichiers comme de fiches, et chaque morceau garde l'historique de ses changements.
- **Ton logiciel de musique, quel qu'il soit** : FL Studio, Ableton Live, Cubase, Reaper, Bitwig… Le projet de chaque morceau s'ouvre d'un clic, et le dossier où tu exportes est surveillé : chaque nouvel export rejoint son morceau, rangé comme version.
- **Production** : un tableau de l'idée à la sortie, des statuts qui avancent tout seuls sur preuve, tes commandes et ton carnet de clients, un outil pour construire tes sets DJ, l'Atelier Suno pour tes prompts.
- **Mastering** : le moteur libre PhaseLimiter, installé d'un clic. Mesures avant et après, écoute à volume égal, un EP entier avec les mêmes réglages. L'original n'est jamais touché.
- **Sorties** : sur les plateformes de ton choix (streaming par un distributeur, SoundCloud, Bandcamp, YouTube), la galerie, le compte à rebours, la prochaine étape et ce qui reste à faire, le planning et ses rappels dans ton agenda, le contrôle avant publication, le dossier à envoyer au distributeur (UPC, ISRC), la discographie en PDF ou en page web.
- **Publier** : un texte par plateforme (SoundCloud, Instagram, story), six tons, tes propres modèles. Tout reste hors ligne.
- **Statistiques** : création, musique, avancement, parcours.
- **Écouter partout** : ta bibliothèque sur ton téléphone et dans Android Auto, avec des morceaux gardés hors ligne. En écoutant, tu gardes, rejettes, changes le statut et poses une note à l'instant précis : elle t'attend sur la forme d'onde du PC.
- **Partager** : un lien privé pour faire écouter un morceau ou une sortie, avec mot de passe et date de fin si tu veux. La personne n'a rien à installer. Elle peut aimer un morceau et laisser un avis à un instant précis, qui arrive en repère sur ta forme d'onde, et tu vois jusqu'où elle a écouté.
- **Rester tranquille** : ta musique reste chez toi. Rien ne part dans le cloud. Ta base est sauvegardée chaque jour, tu peux revenir à une date ou repartir de zéro.
- **Le confort** : seulement les pages qui te servent dans le menu, glisser-déposer avec l'Explorateur et ton logiciel de musique (dans les deux sens), sombre ou clair, couleur d'accent, raccourcis modifiables, l'icône près de l'horloge (Ouvrir, Vérifier une mise à jour, Fermer le logiciel), mise à jour en un clic, travail depuis un autre PC.

## Installer en 2 minutes

1. Va sur la page [Releases](https://github.com/otokonosound/otoko-studio-vault-pro/releases/latest) et télécharge `Otoko-Studio-Vault-Pro-…-installation.exe`.
2. Double-clique dessus. Si Windows affiche « Windows a protégé votre ordinateur », clique sur **Informations complémentaires** puis **Exécuter quand même** : le programme n'a pas de certificat payant.
3. Le logiciel s'ouvre et te guide : ton nom d'artiste, puis ton dossier de musique. C'est tout.

Pas de compte à créer, pas de droits d'administrateur demandés.

**Le tutoriel complet, pas à pas : [TUTORIEL.md](TUTORIEL.md).**

## Ce qu'il te faut

Windows 10 ou 11, 64 bits. C'est tout : le reste s'installe tout seul quand tu en as besoin.

| Pour quoi | Quoi | Comment |
|---|---|---|
| Le téléphone et les liens de partage | [Tailscale](https://tailscale.com/download), gratuit | Le bouton **Tout installer pour moi** de la page Mobile et partage |
| L'écoute légère en données mobiles et les liens | ffmpeg | Le même bouton |
| Le mastering | [PhaseLimiter](https://github.com/ai-mastering/phaselimiter), libre | Le bouton **Installer le moteur** de la page Mastering |

Ces trois outils viennent de leurs sources officielles, et leur hash SHA-256 (ou leur signature) est vérifié avant l'installation.

## Ta vie privée

- Ta musique et ta base restent sur ton PC.
- Ton téléphone se relie à ton PC par Tailscale : un tunnel chiffré entre tes appareils, sans rien ouvrir sur Internet.
- Seuls les liens de partage que tu crées sont visibles sur Internet, et seulement si tu actives le partage public. Un bouton les coupe d'un coup.
- Le logiciel ne contacte GitHub que pour vérifier s'il existe une nouvelle version. Il n'envoie rien te concernant.

## Le code

Ce repo contient tout le code : le logiciel en Rust et Slint, le serveur mobile, l'app Android en Kotlin et l'installateur.

```bat
rem Le logiciel et l'installateur (Rust, Inno Setup 6)
installer\build.cmd

rem Les tests
cargo test --workspace
```

## Licence

Logiciel libre sous licence [GPL-3.0-or-later](LICENSE). Tu peux l'utiliser, l'étudier, le modifier et le redistribuer selon ses termes ; il est fourni sans aucune garantie. Les bibliothèques utilisées et leurs licences sont listées dans `installer/LICENCES.txt`.

Fait par [Otoko](https://soundcloud.com/otokonosound).
