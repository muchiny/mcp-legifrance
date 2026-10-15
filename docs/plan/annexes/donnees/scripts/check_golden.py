import json, sys
g = json.load(open(sys.argv[1], encoding="utf-8"))["normalizer"]
W = "https://www.legifrance.gouv.fr"
n = g["search_loda.json"]; h = n["hits"][0]
assert n["total"] >= 1 and h["kind"] == "loda" and h["id"].startswith("LEGITEXT") and "_" not in h["id"]
assert "<mark>" not in h["title"] and h["source_url"].startswith(W + "/loda/id/") and h["eli_uri"] == h["source_url"]
h = g["search_code.json"]["hits"][0]; assert h["kind"] == "code" and h["articles"][0]["article_id"].startswith("LEGIARTI")
h = g["search_juri.json"]["hits"][0]; assert h["kind"] == "juri" and h["source_url"].startswith(W + "/juri/id/")
n = g["consult_loda.json"]; assert n["text_id"].startswith("LEGITEXT") and n["nature"] == "LOI" and n["articles"][0]["article_id"].startswith("LEGIARTI")
n = g["consult_article.json"]; assert n["num"] == "9" and n["code_title"] == "Code civil" and n["human_readable_citation"] == "Code civil, art. 9"
n = g["consult_juri.json"]; assert n["ecli"].startswith("ECLI:FR:") and n["juridiction"]
h = g["search_constit.json"]["hits"][0]; assert h["kind"] == "constit" and h["source_url"].startswith(W + "/cons/id/")
h = g["search_cetat.json"]["hits"][0]; assert h["kind"] == "cetat" and h["source_url"].startswith(W + "/ceta/id/")
n = g["consult_juri_constit.json"]; assert n["ecli"] == "ECLI:FR:CC:2025:2025.1173.QPC" and n["source_url"] == W + "/cons/id/CONSTEXT000052555275"
assert n["human_readable_citation"].startswith("Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025")
n = g["consult_juri_cetat.json"]; assert n["ecli"].startswith("ECLI:FR:CE") and n["juridiction"] == "Conseil d'État"
h = g["search_cnil.json"]["hits"][0]; assert h["kind"] == "cnil" and h["source_url"].startswith(W + "/cnil/id/")
h = g["search_kali.json"]["hits"][0]; assert h["kind"] == "kali" and h["source_url"].startswith(W + "/conv_coll/id/")
h = g["search_acco.json"]["hits"][0]; assert h["raison_sociale"] and h["idcc"] and h["raison_sociale"] in h["human_readable_citation"]
n = g["consult_cnil.json"]; assert n["nature_delib"] == "Sanction" and n["human_readable_citation"].startswith("CNIL, deliberation n° SAN-2026-002 du")
n = g["consult_kali.json"]; assert n["articles"][0]["article_id"].startswith("KALIARTI")
n = g["consult_acco.json"]; assert n["siret"] and n["themes"] and "text" not in n and n["raison_sociale"] in n["human_readable_citation"]
n = g["consult_article.json[id=KALIARTI000049886059]"]; assert n["source_url"] == W + "/conv_coll/id/KALIARTI000049886059"
for f in ["search_loda.json", "search_juri.json", "search_constit.json", "search_cetat.json"]:
    for h in g[f]["hits"]:
        assert "/eli/" not in (h["eli_uri"] or "")
print("all test_parse assertions hold on golden")
