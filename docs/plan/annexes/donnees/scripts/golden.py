"""Independent re-implementation of the reference normalizers (read from
citations.py by hand) applied to the fixture JSON files, to produce golden
expected outputs for the Rust port. Does NOT import the reference code.

usage: python3 -I golden.py <fixtures_dir> <out_json>
"""
import json
import re
import sys
from datetime import datetime, timezone

WEB = "https://www.legifrance.gouv.fr"
PREF = ("LEGITEXT", "JORFTEXT", "JURITEXT", "CETATEXT", "KALITEXT", "KALIARTI",
        "LEGIARTI", "CNILTEXT", "ACCOTEXT")
MARK = re.compile(r"</?mark>")
MONTHS = ("janvier", "fevrier", "mars", "avril", "mai", "juin", "juillet", "aout",
          "septembre", "octobre", "novembre", "decembre")
KINDS = {"loda": "/loda/id/", "code": "/codes/texte_lc/", "code_article": "/codes/article_lc/",
         "juri": "/juri/id/", "constit": "/cons/id/", "cetat": "/ceta/id/", "jorf": "/jorf/id/",
         "cnil": "/cnil/id/", "kali": "/conv_coll/id/", "acco": "/acco/id/"}
ELI_NOTE = ("Legifrance lf-engine-app returns the ELI field null for legislation; 'eli_uri' is the "
            "stable CID-keyed legifrance.gouv.fr resource URL, not a synthesized /eli/ identifier.")


def strip_marks(t):
    if t is None:
        return None
    return MARK.sub("", t).strip() or None


def strip_suffix(i):
    if not i:
        return i
    if "_" in i:
        h = i.split("_", 1)[0]
        if h.startswith(PREF):
            return h
    return i


def ms_to_date(ms):
    if not isinstance(ms, int):  # note: bool is an int in Python
        return None
    try:
        return datetime.fromtimestamp(ms / 1000, tz=timezone.utc).strftime("%Y-%m-%d")
    except (OverflowError, OSError, ValueError):
        return None


def url(i, kind):
    if not i:
        return None
    p = KINDS.get(kind)
    return WEB + p + i if p else None


def fr_date(ms):
    d = ms_to_date(ms)
    if not d:
        return None
    y, m, dd = d.split("-")
    return f"{int(dd)} {MONTHS[int(m) - 1]} {y}"


def hit(r, fond):
    titles = r.get("titles") or [{}]
    head = titles[0] if titles else {}
    tid = strip_suffix(head.get("id"))
    cid = head.get("cid")
    title = strip_marks(head.get("title"))
    o = {"id": tid, "cid": cid, "title": title, "nature": r.get("nature"), "etat": r.get("etat"),
         "human_readable_citation": title}
    if fond == "CONSTIT":
        o["kind"] = "constit"; u = url(cid or tid, "constit")
    elif fond == "CNIL":
        o["kind"] = "cnil"; u = url(cid or tid, "cnil")
    elif fond == "KALI":
        o["kind"] = "kali"; o["idcc"] = r.get("idcc"); u = url(cid or tid, "kali")
    elif fond == "ACCO":
        o["kind"] = "acco"; o["idcc"] = r.get("idcc"); o["raison_sociale"] = r.get("raisonSociale")
        if title and r.get("raisonSociale"):
            o["human_readable_citation"] = f"{title} ({r['raisonSociale']})"
        u = url(cid or tid, "acco")
    elif fond == "CETAT":
        o["kind"] = "cetat"; u = url(cid or tid, "cetat")
    elif fond.startswith("JURI"):
        o["kind"] = "juri"; u = url(cid or tid, "juri")
    elif fond.startswith("CODE"):
        o["kind"] = "code"; u = url(tid, "code")
        arts = []
        for s in r.get("sections") or []:
            for ex in s.get("extracts") or []:
                if ex.get("id"):
                    sn = strip_marks((ex.get("values") or [""])[0]) or ""
                    arts.append({"article_id": ex.get("id"), "num": ex.get("num"), "snippet": sn[:280]})
        o["articles"] = arts
    else:
        o["kind"] = "loda"; u = url(tid, "loda")
    o["source_url"] = u
    o["eli_uri"] = u
    return o


# Pydantic model field order (models.py) -> serialized order of the tool output.
SEARCHHIT_ORDER = ["kind", "id", "cid", "title", "nature", "etat", "idcc", "raison_sociale",
                   "articles", "eli_uri", "human_readable_citation", "source_url"]


def as_model(d, order, defaults):
    out = {}
    for k in order:
        out[k] = d.get(k, defaults.get(k))
    for k in d:
        if k not in out:
            out[k] = d[k]
    return out


def search(p, fond):
    hits = [hit(r, fond) for r in p.get("results") or []]
    t = p.get("totalResultNumber")
    if not isinstance(t, int):
        t = len(hits)
    return {"total": t, "hits": hits}


def walk(secs, out):
    for s in secs or []:
        for a in s.get("articles") or []:
            if a.get("id"):
                out.append({"article_id": a.get("id"), "num": a.get("num"), "etat": a.get("etat")})
        walk(s.get("sections"), out)


def law(p):
    if not p or not (p.get("id") or p.get("cid")):
        return None
    tid = strip_suffix(p.get("id"))
    toc = []
    walk(p.get("sections"), toc)
    u = url(tid, "loda")
    return {"text_id": tid, "cid": p.get("cid"), "nor": p.get("nor"), "title": p.get("title"),
            "nature": p.get("nature"), "etat": p.get("etat"),
            "date_parution": ms_to_date(p.get("dateParution")), "num_parution": p.get("numParution"),
            "date_debut_version": p.get("dateDebutVersion"), "date_fin_version": p.get("dateFinVersion"),
            "articles": toc, "human_readable_citation": p.get("title"), "source_url": u, "eli_uri": u}


def article(p):
    a = p.get("article")
    if not a or not a.get("id"):
        return None
    titles = a.get("textTitles") or []
    ct = None
    if isinstance(titles, list):
        ce = next((t for t in titles if (t or {}).get("nature") == "CODE"), None)
        e = ce or (titles[0] if titles else None)
        if e:
            ct = e.get("titre")
    num = a.get("num")
    if ct and num:
        c = f"{ct}, art. {num}"
    elif num:
        c = f"Art. {num}"
    else:
        c = ct
    k = "kali" if str(a.get("id")).startswith("KALIARTI") else "code_article"
    u = url(a.get("id"), k)
    return {"article_id": a.get("id"), "num": num, "etat": a.get("etat"), "code_title": ct,
            "full_sections_titre": a.get("fullSectionsTitre"), "date_debut": ms_to_date(a.get("dateDebut")),
            "date_fin": ms_to_date(a.get("dateFin")), "text": a.get("texte"),
            "human_readable_citation": c, "source_url": u, "eli_uri": u}


def constit_cit(t):
    num = t.get("num")
    nat = (t.get("nature") or "").upper()
    dp = fr_date(t.get("dateTexte"))
    if not (num and dp):
        return None
    h = f"Cons. const., decision n° {num}"
    if nat:
        h += f" {nat}"
    h += f" du {dp}"
    m = re.search(r"\[(.+?)\]", t.get("titre") or "")
    if m:
        h += f" - {m.group(1)}"
    return h


def juri(p):
    t = p.get("text")
    if not t or not t.get("id"):
        return None
    o = (t.get("origine") or "").upper()
    k = "constit" if o == "CONSTIT" else "cetat" if o == "CETAT" else "juri"
    u = url(t.get("id"), k)
    c = t.get("titre")
    if o == "CONSTIT":
        c = constit_cit(t) or c
    return {"decision_id": t.get("id"), "ecli": t.get("ecli"), "juridiction": t.get("juridiction"),
            "formation": t.get("formation"), "solution": t.get("solution"),
            "numero_affaire": t.get("numeroAffaire"), "date_texte": ms_to_date(t.get("dateTexte")),
            "nature": t.get("nature"), "title": t.get("titre"), "text": t.get("texte"),
            "human_readable_citation": c, "source_url": u, "eli_uri": u}


def cnil(p):
    t = p.get("text")
    if not t or not t.get("id"):
        return None
    u = url(t.get("id"), "cnil")
    num = t.get("num")
    dp = fr_date(t.get("dateTexte"))
    c = f"CNIL, deliberation n° {num} du {dp}" if (num and dp) else t.get("titre")
    return {"deliberation_id": t.get("id"), "num": num, "nature": t.get("nature"),
            "nature_delib": t.get("natureDelib"), "etat": t.get("etat"),
            "date_texte": ms_to_date(t.get("dateTexte")), "date_publication": ms_to_date(t.get("datePubli")),
            "title": t.get("titre"), "title_long": t.get("titreLong"), "text": t.get("texte"),
            "human_readable_citation": c, "source_url": u, "eli_uri": u}


def kali(p):
    if not p or not (p.get("id") or p.get("cid")):
        return None
    tid = strip_suffix(p.get("id"))
    toc = []
    walk(p.get("sections"), toc)
    for a in p.get("articles") or []:
        if a.get("id"):
            toc.append({"article_id": a.get("id"), "num": a.get("num"), "etat": a.get("etat")})
    u = url(tid, "kali")
    return {"text_id": tid, "cid": p.get("cid"), "nor": p.get("nor"), "title": p.get("title"),
            "nature": p.get("nature"), "etat": p.get("jurisState") or p.get("etat"),
            "date_parution": ms_to_date(p.get("dateParution")), "num_parution": p.get("numParution"),
            "date_debut_version": p.get("dateDebutVersion"), "date_fin_version": p.get("dateFinVersion"),
            "articles": toc, "human_readable_citation": p.get("title"), "source_url": u, "eli_uri": u}


def acco(p):
    a = p.get("acco")
    if not a or not a.get("id"):
        return None
    u = url(a.get("id"), "acco")
    title = a.get("titreTexte")
    comp = a.get("raisonSociale")
    c = f"{title} ({comp})" if title and comp else title
    th = [t.get("libelle") for t in (a.get("themes") or []) if isinstance(t, dict)]
    sy = [s.get("libelle") for s in (a.get("syndicats") or []) if isinstance(s, dict)]
    return {"agreement_id": a.get("id"), "nature": a.get("nature"), "title": title, "raison_sociale": comp,
            "siret": a.get("siret"), "idcc": a.get("codeIdcc"), "code_ape": a.get("codeApe"),
            "secteur": a.get("secteur"), "themes": [t for t in th if t], "syndicats": [x for x in sy if x],
            "date_texte": ms_to_date(a.get("dateTexte")), "date_effet": ms_to_date(a.get("dateEffet")),
            "date_depot": ms_to_date(a.get("dateDepot")),
            "attachment_note": ("Full text is only distributed by Legifrance as a .docx attachment; consult "
                                "source_url for the human-readable page. This tool returns metadata only."), "human_readable_citation": c, "source_url": u, "eli_uri": u}


def trunc(o, n=120):
    if isinstance(o, str) and len(o) > n:
        return o[:n] + f"...<{len(o)} chars, {len(o.encode('utf-8'))} bytes>"
    if isinstance(o, dict):
        return {k: trunc(v, n) for k, v in o.items()}
    if isinstance(o, list):
        if len(o) > 6:
            return [trunc(x, n) for x in o[:3]] + [f"...<{len(o)} items total>"] + [trunc(o[-1], n)]
        return [trunc(x, n) for x in o]
    return o


ORDERS = {
    "act": ["text_id", "cid", "nor", "title", "nature", "etat", "date_parution", "num_parution",
            "date_debut_version", "date_fin_version", "articles", "eli_uri", "human_readable_citation",
            "source_url", "eli_note"],
    "article": ["article_id", "num", "etat", "code_title", "full_sections_titre", "date_debut", "date_fin",
                "text", "byte_size", "eli_uri", "human_readable_citation", "source_url", "eli_note"],
    "decision": ["decision_id", "ecli", "juridiction", "formation", "solution", "numero_affaire", "date_texte",
                 "nature", "title", "text", "byte_size", "eli_uri", "human_readable_citation", "source_url"],
    "deliberation": ["deliberation_id", "num", "nature", "nature_delib", "etat", "date_texte",
                     "date_publication", "title", "title_long", "text", "byte_size", "eli_uri",
                     "human_readable_citation", "source_url"],
    "acco": ["agreement_id", "nature", "title", "raison_sociale", "siret", "idcc", "code_ape", "secteur", "themes",
             "syndicats", "date_texte", "date_effet", "date_depot", "attachment_note", "eli_uri",
             "human_readable_citation", "source_url"],
}
DATASET_NOTE = ("Legifrance via PISTE. Legislation (LODA laws/decrees, codes) and case law (JURI) are "
                "searchable by keyword. The PISTE consult API does NOT expose a populated native ELI for "
                "legislation, so 'eli_uri' carries the stable resolvable Legifrance resource URL (never a "
                "fabricated /eli/ string). Case law carries a native authoritative 'ecli'.")


def main():
    fx, out = sys.argv[1], sys.argv[2]
    ld = lambda n: json.load(open(f"{fx}/{n}", encoding="utf-8"))
    norm, tool = {}, {}
    for f, fond, key in [("search_loda.json", "LODA_DATE", "LODA"), ("search_code.json", "CODE_DATE", "CODE"),
                         ("search_juri.json", "JURI", "JURI"), ("search_constit.json", "CONSTIT", "CONSTIT"),
                         ("search_cetat.json", "CETAT", "CETAT"), ("search_cnil.json", "CNIL", "CNIL"),
                         ("search_kali.json", "KALI", "KALI"), ("search_acco.json", "ACCO", "ACCO")]:
        s = search(ld(f), fond)
        norm[f] = s
        tool[f] = {"fond": key, "query": "<query.strip()>", "total": s["total"],
                   "hits": [as_model(h, SEARCHHIT_ORDER, {"articles": []}) for h in s["hits"]],
                   "eli_note": ELI_NOTE, "dataset_note": DATASET_NOTE}
    for f, fn, kind in [("consult_loda.json", law, "act"), ("consult_article.json", article, "article"),
                        ("consult_juri.json", juri, "decision"), ("consult_juri_constit.json", juri, "decision"),
                        ("consult_juri_cetat.json", juri, "decision"), ("consult_cnil.json", cnil, "deliberation"),
                        ("consult_kali.json", kali, "act"), ("consult_acco.json", acco, "acco")]:
        n = fn(ld(f))
        norm[f] = n
        t = dict(n)
        if kind in ("article", "decision", "deliberation"):
            t["byte_size"] = len((t.get("text") or "").encode("utf-8"))
        if kind in ("act", "article"):
            t["eli_note"] = ELI_NOTE
        tool[f] = as_model(t, ORDERS[kind], {})
    a = ld("consult_article.json"); a["article"]["id"] = "KALIARTI000049886059"
    norm["consult_article.json[id=KALIARTI000049886059]"] = article(a)
    meta = {"generated_by": "scratchpad/scripts/golden.py (re-implementation independante, n'importe pas la reference)",
            "reference": "matematicsolutions/fr-eli-mcp@d3b5b13 tests/fixtures/*.json",
            "normalizer": "sortie des fonctions citations.normalize_* (dict brut, avant modele pydantic)",
            "tool_output": "structuredContent attendu de l'outil (ordre des champs des modeles models.py, defauts appliques, byte_size, eli_note/dataset_note)"}
    json.dump({"_meta": meta, "normalizer": norm, "tool_output": tool}, open(out, "w", encoding="utf-8"),
              ensure_ascii=False, indent=1)
    print("ok")


main()
