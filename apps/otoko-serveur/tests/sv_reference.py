"""Réponses de référence du code de Studio Vault (composer_vault/mobile/library.py), sur une
copie de vault.db, pour les comparer à celles d'OTOKO. Lecture seule ; à lancer avec -I -B
(rien n'est écrit dans le dossier de Studio Vault).

python -I -B sv_reference.py <dossier Studio Vault> <copie de vault.db>
"""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, sys.argv[1])
from composer_vault.mobile import library as L  # noqa: E402

con = L.connect(Path(sys.argv[2]))
LISTS = {
    "added": {},
    "title_all": {"sort": "title", "scope": "all"},
    "published": {"sort": "published"},
    "bpm": {"sort": "bpm"},
    "key": {"sort": "key"},
    "plays": {"sort": "plays"},
    "q_8A": {"q": "8A"},
    "q_word": {"q": "love"},
    "q_bpm": {"q": "124"},
    "compatible": {"compatible_with": 1837},
    "nature": {"nature": "original,remix", "scope": "all"},
    "status": {"status": "done,released"},
    "keys": {"key": "8A,9A"},
    "bpm_range": {"bpm_min": 120, "bpm_max": 128},
    "page": {"limit": 20, "offset": 40},
}
out = {"lists": {k: L.list_tracks(con, **v) for k, v in LISTS.items()}}
every = [t["id"] for t in L.list_tracks(con, scope="all", limit=100000)]
out["tracks"] = {str(i): L.get_track(con, i) for i in every}
out["releases"] = L.list_releases(con)
out["release_tracks"] = {str(r["id"]): L.release_tracks(con, r["id"]) for r in out["releases"]}
out["covers"] = {str(i): [[k, str(p)] for k, p in L.cover_sources(con, i)] for i in every}
out["release_covers"] = {str(r["id"]): [[k, str(p)] for k, p in L.release_cover_sources(con, r["id"])]
                         for r in out["releases"]}
out["views"] = L.list_views(con)
out["summary"] = L.summary(con)
sys.stdout.buffer.write(json.dumps(out, ensure_ascii=False, default=str).encode("utf-8"))
