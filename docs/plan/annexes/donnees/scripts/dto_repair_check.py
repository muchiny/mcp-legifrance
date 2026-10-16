"""Check the proposed mojibake repair strategies on every fixture.

Body-level: decode file as UTF-8 -> if (no char > U+00FF) and (mojibake pattern present):
            bytes = [ord(c) for c in body] -> strict UTF-8 decode -> must parse as JSON and have no
            residual pattern. Field-level: same rule applied per string value and per object key.
Also checks: identity on clean fixtures, idempotence, equality body-level == field-level, and the
NBSP trap (normalizing U+00A0 before repair breaks 'Ã\\xa0' = 'à').

Usage: python3 -I dto_repair_check.py <fixture.json> [...]
"""
import json
import re
import sys

# generic "UTF-8 read as ISO-8859-1" pattern: lead byte C2..F4 then continuation 80..BF, or C1 controls
PAT = re.compile(r"[Â-ô][\u0080-¿]|[\u0080-\u009f]")


def field_repair(s):
    if not PAT.search(s):
        return s, False
    if any(ord(c) > 0xFF for c in s):
        return s, False
    try:
        r = bytes(ord(c) for c in s).decode("utf-8")
    except UnicodeDecodeError:
        return s, False
    if PAT.search(r):
        return s, False
    return r, True


def walk(v, stats):
    if isinstance(v, dict):
        out = {}
        for k, x in v.items():
            k2, ch = field_repair(k)
            stats["keys"] += ch
            out[k2] = walk(x, stats)
        return out
    if isinstance(v, list):
        return [walk(x, stats) for x in v]
    if isinstance(v, str):
        r, ch = field_repair(v)
        stats["vals"] += ch
        return r
    return v


def body_repair(body):
    if any(ord(c) > 0xFF for c in body):
        return None, "has_char_above_FF"
    if not PAT.search(body):
        return None, "no_pattern"
    try:
        r = bytes(ord(c) for c in body).decode("utf-8")
    except UnicodeDecodeError as e:
        return None, f"utf8_error@{e.start}"
    if PAT.search(r):
        return None, "residual"
    return r, "repaired"


def main():
    for p in sys.argv[1:]:
        body = open(p, encoding="utf-8").read()
        name = p.rsplit("/", 1)[-1]
        stats = {"keys": 0, "vals": 0}
        fl = walk(json.loads(body), stats)
        br, why = body_repair(body)
        eq = None
        if br is not None:
            eq = json.loads(br) == fl
        # idempotence
        st2 = {"keys": 0, "vals": 0}
        walk(fl, st2)
        # NBSP trap: normalize U+00A0 -> ' ' before repair
        trap = 0
        def trapwalk(v):
            nonlocal trap
            if isinstance(v, dict):
                for k, x in v.items():
                    trapwalk(x)
            elif isinstance(v, list):
                for x in v:
                    trapwalk(x)
            elif isinstance(v, str) and PAT.search(v):
                r1, ok1 = field_repair(v)
                r2, ok2 = field_repair(v.replace(" ", " "))
                if ok1 and not ok2:
                    trap += 1
        trapwalk(json.loads(body))
        print(f"{name}: field_level repaired vals={stats['vals']} keys={stats['keys']} | body_level={why}"
              f"{'' if eq is None else ' equal_to_field_level=' + str(eq)} | second_pass_changes={st2['vals'] + st2['keys']}"
              f" | fields_broken_if_nbsp_normalized_first={trap}")


if __name__ == "__main__":
    main()
