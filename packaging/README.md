# Packaging

Everything here is refreshed by `.github/workflows/release.yml` after a tag
is published — don't edit versions or hashes by hand.

| Path | Channel | How it's updated |
| --- | --- | --- |
| `npm/` | `@mulham28/trelloc` on npm | version bumped by release-please; published via npm Trusted Publishing (OIDC) |
| Homebrew | `mulhamna/homebrew-tap` → `Formula/trelloc.rb` | release workflow writes the formula and pushes to the tap |
| Scoop | `mulhamna/scoop-bucket` → `bucket/trelloc.json` | release workflow writes the manifest and pushes to the bucket |
