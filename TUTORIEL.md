# Tutoriel : de zéro à ta musique sur ton téléphone

Aucune connaissance technique nécessaire. Compte **2 minutes** pour le logiciel, et quelques minutes de plus si tu veux ta musique sur ton téléphone : le logiciel installe tout ce qu'il lui faut.

## 1. Installer le logiciel

1. Sur la page des [versions](https://github.com/otokonosound/otoko-studio-vault-pro/releases/latest), télécharge `Otoko-Studio-Vault-Pro-…-installation.exe`.
2. Double-clique sur le fichier.
3. Si Windows affiche « Windows a protégé votre ordinateur » : clique sur **Informations complémentaires**, puis **Exécuter quand même**.
4. Laisse l'installation se faire, puis clique sur **Terminer**. Le logiciel s'ouvre.

## 2. Le réglage de départ

L'accueil te guide en deux gestes :

1. **Ton nom d'artiste** : tape-le. Il s'affichera sur tes liens de partage.
2. **Ton dossier de musique** : clique sur **+ Ajouter mon dossier de musique** et choisis-le.

Tes morceaux apparaissent tout de suite. L'analyse (tempo, tonalité, loudness) continue en fond : tu peux déjà t'en servir. Tes fichiers ne sont **jamais déplacés ni modifiés**.

## 3. Retrouver ta musique

- **Ctrl+K** ouvre la palette : tape un bout de titre et appuie sur Entrée pour écouter.
- La page **Bibliothèque** trie par tempo, tonalité, date ou statut.
- **Espace** lance ou met en pause.

## 4. Ta musique sur ton téléphone (Android)

Ouvre la page **Mobile et partage** du logiciel.

**Sur ton PC, un seul bouton : Tout installer pour moi.** Il installe Tailscale (ce qui relie tes appareils, de façon chiffrée) et ffmpeg, puis démarre le serveur. Une page s'ouvre pour te connecter à Tailscale : un compte gratuit Google ou Microsoft suffit.

Quand c'est fini, deux codes s'affichent sur la page. **Sur ton téléphone** :

1. **Scanne le premier code** : il ouvre Tailscale sur le Play Store. Installe-le et connecte-toi avec **le même compte** que sur le PC.
2. **Scanne le second code** : il ouvre ta musique dans le navigateur.
3. Touche **App Android**, installe Otoko Vault (Android demande d'autoriser l'installation : accepte).
4. Touche **Ouvrir l'app** : l'adresse de ton PC est déjà remplie. Ta musique arrive.

## 5. Dans ta voiture (Android Auto)

L'app ne vient pas du Play Store : Android Auto la cache tant que tu ne l'as pas autorisée. À faire une seule fois sur le téléphone :

1. Ouvre les réglages d'Android Auto : Paramètres › Applis › Android Auto.
2. Tout en bas, touche **Version**, puis touche **10 fois** « Version et informations sur les autorisations », et accepte.
3. Dans le menu **⋮** en haut à droite, ouvre les paramètres pour les développeurs et coche **Sources inconnues**. Laisse-la cochée.
4. Rebranche le téléphone dans la voiture. Otoko Vault est dans le lanceur.

## 6. Faire écouter un morceau à quelqu'un

1. Page **Mobile et partage**, clique sur **Partage public…** (une seule fois : Tailscale te demande d'autoriser).
2. **Nouveau lien** : choisis un morceau ou une sortie, ajoute un mot de passe ou une date de fin si tu veux.
3. **Copier le lien** et envoie-le. La personne écoute dans son navigateur, sans rien installer.

Tu vois qui a écouté, et tu peux couper un lien quand tu veux.

## Fermer le logiciel, le rouvrir

- **La croix** ferme la fenêtre, mais le logiciel reste près de l'horloge (la musique continue).
- **Un clic sur l'icône** rouvre la fenêtre.
- **Clic droit sur l'icône** : Ouvrir, Vérifier une mise à jour, Fermer le logiciel.

Si l'icône n'est pas visible, regarde sous la petite flèche **^** à côté de l'horloge. Tu préfères que la croix ferme tout ? Décoche l'option dans **Réglages › Général**.

## Revenir en arrière

Ta base (tes fiches, notes et sorties, pas tes fichiers audio) est sauvegardée chaque jour. Dans **Réglages › Analyse et sauvegarde** :

- **Restaurer…** : remet ta base comme elle était à cette date. Une sauvegarde est faite juste avant, tu peux donc changer d'avis.
- **Réinitialiser ma base…** : repart de zéro. Tes fichiers audio ne sont pas touchés, et là aussi une sauvegarde est faite avant.

## Mettre à jour

Le logiciel te prévient quand une nouvelle version existe : un clic, il s'installe et se relance. Pour vérifier à la main : clic droit sur l'icône près de l'horloge, **Vérifier une mise à jour**.

Le téléphone reçoit la sienne depuis ton PC : un bandeau te propose de l'installer.

## Ça ne marche pas ?

| Ce que tu vois | Quoi faire |
|---|---|
| Windows bloque l'installation | **Informations complémentaires**, puis **Exécuter quand même** |
| « Rien ne répond à cette adresse » sur le téléphone | Vérifie que Tailscale est connecté sur le téléphone **et** sur le PC, avec le même compte |
| Pas de son sur le PC | Vérifie la sortie dans les paramètres de son de Windows |
| Le morceau ne se lit pas sur le téléphone | Clique sur **Tout installer pour moi** dans la page Mobile et partage |
| L'app n'apparaît pas dans la voiture | Vérifie la case **Sources inconnues** (étape 5), puis rebranche le téléphone |
| Le mastering ne démarre pas | Clique sur **Installer le moteur** en haut de la page Mastering |

Une question ou un souci : ouvre un [ticket](https://github.com/otokonosound/otoko-studio-vault-pro/issues).
