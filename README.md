Pictures for the Mac Japanese readings PR (lucacadalora/poni#70, branch ja-readings-mac). Keep while the PR body links them.

- `mac-ja-readings-before-after.png`: the board in the PR body.
- `before-0.1.20-*`: the Mac's offscreen render (`poni --render`, mac.yml) of the Music page with the Japanese test song, built from main at 0151527 (Release 0.1.20, dictionary built in) plus only that render scene, on a throwaway branch (deleted).
- `after-26-*` (add-on on), `after-27-*` (the offer), `after-28-*` (downloading), `after-29-*` (failed, no Add-ons tab): the same render from the PR's last commit. `after-26` is pixel-identical to `before-0.1.20` in all six theme and notch variants.
- `smoke-*`: the real window on CI's macOS 15 runner (tools/mac-smoke.sh): the offer, then the add-on downloaded from GitHub and the song read again.
