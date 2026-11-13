"""compare.py <before.txt> <after.txt>: the pages whose panels changed between two runs of run.sh."""
import re
import statistics as st
import sys


def parse(f):
    d, cur, times = {}, None, {}
    for l in open(f, encoding="utf8", errors="replace"):
        m = re.match(r"^(\d+): (.*) p(\d+): (\d+) panels in ([\d.]+)(ms|s|µs)", l)
        if m:
            cur = int(m.group(1))
            times[cur] = float(m.group(5)) * {"s": 1000, "ms": 1, "µs": 0.001}[m.group(6)]
            d[cur] = [m.group(2)[:44] + " p" + m.group(3), m.group(4), ""]
        elif cur is not None and l.strip().startswith("= "):
            d[cur][2] = l.strip()
    return d, times


b, tb = parse(sys.argv[1])
a, ta = parse(sys.argv[2])
print(len(b), "pages before,", len(a), "after")
changed = 0
for k in sorted(b):
    now = a.get(k, ["", "missing", ""])
    if b[k][1:] != now[1:]:
        changed += 1
        print(k, "(sheet %d, place %d)" % (k // 8 + 1, k % 8 + 1), b[k][0], b[k][1], "->", now[1], "|", now[2][:160])
print(changed, "changed")
if tb and ta:
    print("ms before: mean %.0f, max %.0f; after: mean %.0f, max %.0f" % (st.mean(tb.values()), max(tb.values()), st.mean(ta.values()), max(ta.values())))
