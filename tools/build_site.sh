#!/usr/bin/env bash
# Assemble the website (served by GitHub Pages, see .github/workflows/pages.yml)
# into _site/: the page and its screenshots from site/, plus docs/brand as brand/.
#
# Preview locally:  tools/build_site.sh && python3 -m http.server -d _site 8000
# New screenshots:  python3 tools/site_shots.py SELFTEST_DIR classic|oyen
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf _site
mkdir -p _site
cp -r site/. _site/
cp -r docs/brand _site/brand
rm -f _site/brand/README.md
echo "_site ready: $(du -sh _site | cut -f1)"
