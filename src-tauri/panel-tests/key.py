"""Adds pages to the answer key (key.jsonl) from a run of run.sh.

key.py right <run.txt> 0-7 12 15     these pages' panels are right as found: keep them as the answer
key.py count <run.txt> 9=5 14=0      these are wrong as found; the right number of stops is known

Numbers are the ones each page starts with in the run's text (sheet n holds pages 8(n-1) to 8n-1).
A page already in the key is replaced.
"""
import json
import os
import re
import sys

here = os.path.dirname(os.path.abspath(__file__))
key_file = os.path.join(here, "key.jsonl")
mode, run, picks = sys.argv[1], sys.argv[2], sys.argv[3:]

found, cur = {}, None
for l in open(run, encoding="utf8", errors="replace"):
    m = re.match(r"^(\d+): ", l)
    if m:
        cur = int(m.group(1))
    elif cur is not None and l.strip().startswith("key "):
        found[cur] = json.loads(l.strip()[4:])

key = {}
if os.path.exists(key_file):
    for l in open(key_file, encoding="utf8"):
        if l.strip():
            e = json.loads(l)
            key[(e["file"], e["page"])] = e

for pick in picks:
    if mode == "right":
        a, _, b = pick.partition("-")
        for n in range(int(a), int(b or a) + 1):
            e = found[n]
            key[(e["file"], e["page"])] = e
    else:
        n, count = pick.split("=")
        e = found[int(n)]
        key[(e["file"], e["page"])] = {"file": e["file"], "page": e["page"], "rtl": e["rtl"], "count": int(count)}

with open(key_file, "w", encoding="utf8", newline="\n") as f:
    for e in sorted(key.values(), key=lambda e: (e["file"], e["page"])):
        f.write(json.dumps(e, ensure_ascii=False) + "\n")
print(len(key), "pages in the key")
