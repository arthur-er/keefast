#!/usr/bin/env bash
# PROTOTYPE — regenerate shots/ from manifest.js. Needs the static server on :4173 and agent-browser.
set -euo pipefail
cd "$(dirname "$0")"
S=kf-shoot
BASE=${BASE:-http://127.0.0.1:4173}
rm -f shots/*.png
agent-browser --session $S open "$BASE/" >/dev/null
agent-browser --session $S set viewport 1280 800 >/dev/null
node -e '
const {MANIFEST, shotName} = require("./manifest.js");
for (const s of MANIFEST) for (const [v, [, states]] of Object.entries(s.variants))
  for (const [q] of states) console.log(`screens/${s.key}.html?variant=${v}${q ? "&" + q : ""}&shot=1 shots/${shotName(s.key, v, q)}`);
' | while read -r url out; do
  agent-browser --session $S open "$BASE/$url" >/dev/null
  sleep 1.2
  agent-browser --session $S screenshot "$out" >/dev/null
  echo "$out"
done
agent-browser --session $S close >/dev/null
