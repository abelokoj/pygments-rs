"""Turn a CI log into GitHub annotations, which (unlike job logs) anyone can read
through the public API: /repos/OWNER/REPO/check-runs/JOB_ID/annotations.

usage: python .github/annotate.py LEVEL TITLE FILE   (LEVEL: error|warning|notice)
"""

import sys

CHUNK = 30_000  # keep each annotation well under GitHub's message limit
MAX = 9  # GitHub keeps at most 10 annotations of a level per step

level, title, path = sys.argv[1:4]
try:
    text = open(path, encoding='utf-8', errors='replace').read()
except FileNotFoundError:
    sys.exit(0)
# Cargo output: keep only diagnostics and test failures when the log is long.
if len(text) > CHUNK * MAX:
    keep = [l for l in text.splitlines() if not l.lstrip().startswith(('Compiling', 'Checking', 'Downloaded', 'Running'))]
    text = '\n'.join(keep)
for i in range(0, min(len(text), CHUNK * MAX), CHUNK):
    body = text[i:i + CHUNK].replace('%', '%25').replace('\r', '%0D').replace('\n', '%0A')
    print(f'::{level} title={title} ({i // CHUNK + 1})::{body}')
