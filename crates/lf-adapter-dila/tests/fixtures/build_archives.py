#!/usr/bin/env python3
"""Reconstruit les deux mini-archives LEGI de test à partir des fixtures et de leurs sidecars.

Les fichiers XML et .dat de `xml/` et `suppression/` sont des membres RÉELS des archives de la
DILA (octets identiques, voir chaque `.meta.toml`). Ce script ne fait que les ré-empaqueter, avec
la même structure que les vraies archives (`legi/global/...`, préfixe `AAAAMMJJ-HHMMSS/` pour un
incrément), pour qu'un test d'ingestion en flux (flate2, tar, quick-xml) ait de quoi lire sans
télécharger 1 Go. Aucune donnée n'est inventée.

Usage (stdlib seule, Python 3.11+) :
    python3 -I build_archives.py          # (re)génère archives/*.tar.gz et leurs sidecars
    python3 -I build_archives.py --check  # vérifie sans rien écrire (code de sortie 1 si écart)

Reproductibilité : le TAR (non compressé) est déterministe, son sha256 est consigné dans le
sidecar de l'archive (`sha256_tar`). Le sha256 du .tar.gz dépend de l'implémentation de zlib
(zlib, zlib-ng) et n'est donc qu'indicatif : `--check` compare le TAR décompressé.
"""
import datetime as dt
import gzip
import hashlib
import io
import os
import sys
import tarfile
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "archives")
UNAME = "extract"  # propriétaire observé dans les vraies archives
UID = 1000

# Lien symbolique réel du global (arbre eli/), à conserver pour tester la tolérance aux liens.
SYMLINK = (
    "legi/global/eli/loi_organique/2004/2/27/2004-192",
    "DOMX0300085L",
    "2025-05-18T20:54:00Z",
)

ARCHIVES = {
    "global": dict(
        file="legi-global-mini.tar.gz",
        reel="Freemium_legi_global_20250713-140000.tar.gz",
        prefix="",
        dir_mtime="2025-07-13T12:00:00Z",
    ),
    "incr": dict(
        file="legi-incr-20261028-mini.tar.gz",
        reel="LEGI_20261028-212713.tar.gz",
        prefix="20261028-212713/",
        dir_mtime="2026-10-28T20:27:00Z",
    ),
}


def epoch(iso):
    return int(dt.datetime.strptime(iso, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc).timestamp())


def sidecars():
    """Parcourt les sidecars et rend [(sidecar dict, chemin du fichier fixture)]."""
    found = []
    for base in ("xml", "suppression"):
        for root, _, names in os.walk(os.path.join(HERE, base)):
            for n in names:
                if not n.endswith(".meta.toml"):
                    continue
                with open(os.path.join(root, n), "rb") as fh:
                    meta = tomllib.load(fh)
                stem = n[: -len(".meta.toml")]
                cands = [os.path.join(root, stem + ext) for ext in (".xml", ".dat")]
                path = next((c for c in cands if os.path.exists(c)), None)
                if path is None:
                    sys.exit("sidecar sans fichier : " + os.path.join(root, n))
                found.append((meta, path))
    return found


def build_tar(origin, spec, items):
    """Construit le TAR (octets) d'une archive. `items` : [(meta, chemin)] de cette origine."""
    entries = {}  # chemin dans l'archive -> ('f', octets, mtime) | ('l', cible, mtime)
    for meta, path in items:
        with open(path, "rb") as fh:
            data = fh.read()
        entries[meta["member"]] = ("f", data, epoch(meta["member_mtime"]))
    if origin == "global":
        entries[SYMLINK[0]] = ("l", SYMLINK[1], epoch(SYMLINK[2]))
    dirs = {}
    for name, (_, _, mtime) in entries.items():
        parts = name.split("/")
        for i in range(1, len(parts)):
            d = "/".join(parts[:i]) + "/"
            dirs[d] = max(dirs.get(d, 0), mtime)
    if spec["prefix"]:
        dirs[spec["prefix"]] = max(dirs.get(spec["prefix"], 0), epoch(spec["dir_mtime"]))
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.GNU_FORMAT) as tf:
        # Ordre : lexicographique, ce qui place chaque dossier avant son contenu.
        for name in sorted(set(dirs) | set(entries), key=lambda s: s.encode()):
            ti = tarfile.TarInfo(name)
            ti.uid = ti.gid = UID
            ti.uname = ti.gname = UNAME
            if name in dirs and name not in entries:
                ti.type = tarfile.DIRTYPE
                ti.mode = 0o755
                ti.mtime = dirs[name]
                tf.addfile(ti)
                continue
            kind, payload, mtime = entries[name]
            ti.mtime = mtime
            if kind == "l":
                ti.type = tarfile.SYMTYPE
                ti.linkname = payload
                ti.mode = 0o777
                tf.addfile(ti)
            else:
                ti.size = len(payload)
                ti.mode = 0o644
                tf.addfile(ti, io.BytesIO(payload))
    return buf.getvalue(), len(entries), len(dirs)


def gzip_bytes(raw):
    out = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=out, compresslevel=9, mtime=0) as gz:
        gz.write(raw)
    return out.getvalue()


def sidecar_text(origin, spec, raw, gz, n_files, n_dirs):
    return "\n".join(
        [
            'objet = "mini-archive de test reconstruite à partir de membres réels (voir build_archives.py)"',
            'fonds = "LEGI"',
            'origin = "%s"' % origin,
            'imite = "%s"' % spec["reel"],
            'source = "https://echanges.dila.gouv.fr/OPENDATA/LEGI/%s"' % spec["reel"],
            'archive_prefix = "%s"' % spec["prefix"],
            "membres_fichiers = %d" % n_files,
            "membres_dossiers = %d" % n_dirs,
            "bytes_tar = %d" % len(raw),
            'sha256_tar = "%s"' % hashlib.sha256(raw).hexdigest(),
            "bytes_gz = %d" % len(gz),
            'sha256_gz = "%s"  # indicatif : dépend de l\'implémentation de zlib' % hashlib.sha256(gz).hexdigest(),
            'captured_at = "2026-10-29"',
            'transform = "ré-empaquetage GNU tar + gzip niveau 9, mtime gzip 0 ; contenu des membres inchangé"',
            "redactions = []",
            "known_issues = %s"
            % ('["lien symbolique pendant (cible hors archive) : eli/loi_organique/2004/2/27/2004-192"]' if origin == "global" else "[]"),
            'tool = "build_archives.py (unité V1-16, T-M9-01)"',
            'license = "Licence Ouverte 2.0 — source : DILA, base LEGI (Légifrance)"',
            "",
        ]
    )


def main():
    check = "--check" in sys.argv[1:]
    all_items = sidecars()
    bad = 0
    for meta, path in all_items:  # chaque fixture doit être octet pour octet celle du sidecar
        with open(path, "rb") as fh:
            sha = hashlib.sha256(fh.read()).hexdigest()
        if sha != meta["sha256_file"] or sha != meta["sha256_raw"]:
            print("ECART sha256 :", os.path.relpath(path, HERE))
            bad += 1
    os.makedirs(OUT, exist_ok=True)
    for origin, spec in ARCHIVES.items():
        items = [(m, p) for m, p in all_items if m.get("in_mini_archive") and m["origin"] == origin]
        raw, n_files, n_dirs = build_tar(origin, spec, items)
        target = os.path.join(OUT, spec["file"])
        side = target[: -len(".tar.gz")] + ".meta.toml"
        if check:
            with open(target, "rb") as fh:
                old = gzip.decompress(fh.read())
            if old != raw:
                print("ECART archive :", spec["file"])
                bad += 1
            else:
                print("ok", spec["file"], hashlib.sha256(raw).hexdigest(), "(%d fichiers, %d dossiers)" % (n_files, n_dirs))
            continue
        gz = gzip_bytes(raw)
        with open(target, "wb") as fh:
            fh.write(gz)
        with open(side, "w", encoding="utf-8") as fh:
            fh.write(sidecar_text(origin, spec, raw, gz, n_files, n_dirs))
        print("écrit", spec["file"], len(gz), "octets,", n_files, "fichiers,", n_dirs, "dossiers")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
