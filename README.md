Pictures for the Mac Japanese readings PR (lucacadalora/poni#70, branch ja-readings-mac): the add-on, and the Mac's Add-ons tab with it as its one add-on. Keep while the PR body links them.

- `mac-ja-readings-before-after.png`: the board in the PR body.
- `before-0.1.21-29-*`: the Mac's offscreen render (`poni --render`, mac.yml) of the Music page with the Japanese test song, built from main at 197ed5d (Release 0.1.21, dictionary built in, no Add-ons tab on the Mac) plus only that render scene, on a throwaway branch (deleted).
- `after-29-*` (add-on on), `after-30-*` (the offer), `after-31-*` (downloading), `after-32-*` (failed): the same render from the PR's last commit.
- `after-33-*` (Add-ons tab, off), `after-34-*` (downloading), `after-35-*` (on): the Mac's Add-ons tab, listing Japanese readings only.
- `smoke-*`: the real window on CI's macOS 15 runner (tools/mac-smoke.sh): the offer, the add-on downloaded from GitHub and the song read again, the Add-ons tab with it on, and switched off there.
