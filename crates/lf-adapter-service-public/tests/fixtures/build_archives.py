#!/usr/bin/env python3
"""Reconstruit les deux mini-archives Service-Public de test à partir des vraies archives du jour.

Les fichiers de `xml/part/` et `xml/pro/` sont des entrées RÉELLES de `vosdroits-latest.zip`
(octets identiques une fois décompressés). Ce script recopie, pour chacune, l'en-tête local et les
données compressées de la vraie archive tels quels, puis l'enregistrement du répertoire central
dont seul le décalage de l'en-tête local change, et écrit une fin de répertoire neuve. Rien n'est
recompressé ni inventé : le format est celui que sert la DILA (`deflate`, champs extra `UT` et
`ux`, ni zip64, ni descripteur, ni commentaire), sans télécharger 24 Mo dans chaque test.

Usage (stdlib seule, Python 3.11+) :
    python3 -I build_archives.py --from PART_ZIP PRO_ZIP  # (re)génère archives/*.zip
    python3 -I build_archives.py --check                  # vérifie sans rien écrire (sortie 1 si écart)

Reproductibilité : la construction ne recompresse rien, donc la même vraie archive donne les mêmes
octets, quelle que soit la bibliothèque zlib de la machine. `--check` n'a pas besoin des vraies
archives : il relit les mini-archives et les compare aux fichiers de `xml/`.
"""
import os
import struct
import sys
import zipfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
XML = os.path.join(HERE, "xml")
OUT = os.path.join(HERE, "archives")
AUDIENCES = ("part", "pro")
ARCHIVE = "{aud}-vosdroits-mini.zip"

LOCAL_SIG = 0x04034B50
CENTRAL_SIG = 0x02014B50
EOCD_SIG = 0x06054B50
LOCAL_LEN = 30
CENTRAL_LEN = 46
EOCD_LEN = 22


def wanted(aud):
    """Noms des entrées à garder : les fichiers de xml/<aud>/, triés."""
    return sorted(os.listdir(os.path.join(XML, aud)))


def fixture(aud, name):
    with open(os.path.join(XML, aud, name), "rb") as fh:
        return fh.read()


def central_records(raw):
    """Rend {nom: (octets de l'enregistrement central, décalage de l'en-tête local)}."""
    if len(raw) < EOCD_LEN or struct.unpack_from("<I", raw, len(raw) - EOCD_LEN)[0] != EOCD_SIG:
        sys.exit("fin de répertoire absente des 22 derniers octets (commentaire d'archive ?)")
    _, disk, cd_disk, n_disk, n_total, cd_size, cd_off, comment = struct.unpack_from(
        "<IHHHHIIH", raw, len(raw) - EOCD_LEN
    )
    if (disk, cd_disk, comment) != (0, 0, 0) or n_disk != n_total or 0xFFFF in (n_total,):
        sys.exit("fin de répertoire inattendue (disques, zip64 ou commentaire)")
    records, pos = {}, cd_off
    for _ in range(n_total):
        if struct.unpack_from("<I", raw, pos)[0] != CENTRAL_SIG:
            sys.exit("signature centrale absente à %d" % pos)
        name_len, extra_len, comment_len = struct.unpack_from("<HHH", raw, pos + 28)
        local_off = struct.unpack_from("<I", raw, pos + 42)[0]
        end = pos + CENTRAL_LEN + name_len + extra_len + comment_len
        name = raw[pos + CENTRAL_LEN : pos + CENTRAL_LEN + name_len].decode("ascii")
        records[name] = (raw[pos:end], local_off)
        pos = end
    if pos != cd_off + cd_size:
        sys.exit("taille du répertoire central incohérente")
    return records


def local_record(raw, name, central):
    """Rend l'en-tête local et les données compressées de l'entrée, vérifiés contre le central."""
    rec, off = central
    flags, method = struct.unpack_from("<HH", rec, 8)
    crc, csize, usize = struct.unpack_from("<III", rec, 16)
    if flags != 0 or method != 8:
        sys.exit("%s : drapeaux %#x ou méthode %d hors du format mesuré" % (name, flags, method))
    if struct.unpack_from("<I", raw, off)[0] != LOCAL_SIG:
        sys.exit("%s : signature locale absente" % name)
    lflags, lmethod = struct.unpack_from("<HH", raw, off + 6)
    lcrc, lcsize, lusize, name_len, extra_len = struct.unpack_from("<IIIHH", raw, off + 14)
    if (lflags, lmethod, lcrc, lcsize, lusize) != (flags, method, crc, csize, usize):
        sys.exit("%s : en-tête local et central divergent" % name)
    if raw[off + LOCAL_LEN : off + LOCAL_LEN + name_len].decode("ascii") != name:
        sys.exit("%s : nom local divergent" % name)
    data_off = off + LOCAL_LEN + name_len + extra_len
    data = raw[data_off : data_off + csize]
    plain = zlib.decompressobj(-15).decompress(data)
    if len(plain) != usize or zlib.crc32(plain) != crc:
        sys.exit("%s : taille ou CRC-32 faux après décompression" % name)
    return raw[off:data_off] + data, plain


def build(aud, real_path):
    with open(real_path, "rb") as fh:
        raw = fh.read()
    records = central_records(raw)
    names = wanted(aud)
    missing = [n for n in names if n not in records]
    if missing:
        sys.exit("%s : absentes de la vraie archive : %s" % (aud, ", ".join(missing)))
    # Ordre de la vraie archive, pour garder sa disposition.
    names.sort(key=lambda n: records[n][1])
    body, central = bytearray(), bytearray()
    for name in names:
        local, plain = local_record(raw, name, records[name])
        if plain != fixture(aud, name):
            sys.exit("%s/%s : diffère du fichier de xml/ (refaire les fixtures)" % (aud, name))
        rec = bytearray(records[name][0])
        struct.pack_into("<I", rec, 42, len(body))
        body += local
        central += rec
    eocd = struct.pack("<IHHHHIIH", EOCD_SIG, 0, 0, len(names), len(names), len(central), len(body), 0)
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, ARCHIVE.format(aud=aud)), "wb") as fh:
        fh.write(bytes(body + central + eocd))


def extra_ids(extra):
    ids, pos = [], 0
    while pos + 4 <= len(extra):
        hid, size = struct.unpack_from("<HH", extra, pos)
        ids.append(hid)
        pos += 4 + size
    return ids


def check(aud):
    path = os.path.join(OUT, ARCHIVE.format(aud=aud))
    problems = []
    with open(path, "rb") as fh:
        raw = fh.read()
    central_records(raw)  # format de la fin de répertoire et du répertoire central
    with zipfile.ZipFile(path) as zf:
        infos = zf.infolist()
        if sorted(i.filename for i in infos) != wanted(aud):
            problems.append("entrées différentes des fichiers de xml/%s/" % aud)
        for info in infos:
            if (info.compress_type, info.flag_bits, info.create_system) != (8, 0, 3):
                problems.append("%s : méthode, drapeaux ou système inattendus" % info.filename)
            if extra_ids(info.extra) != [0x5455, 0x7875]:
                problems.append("%s : champs extra inattendus" % info.filename)
            if zf.read(info) != fixture(aud, info.filename):  # read() vérifie aussi le CRC-32
                problems.append("%s : contenu différent de xml/%s/" % (info.filename, aud))
    return problems


def main(argv):
    if argv[:1] == ["--from"] and len(argv) == 3:
        for aud, real in zip(AUDIENCES, argv[1:]):
            build(aud, real)
        argv = ["--check"]
    if argv != ["--check"]:
        sys.exit(__doc__)
    problems = [p for aud in AUDIENCES for p in check(aud)]
    for p in problems:
        print(p, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
