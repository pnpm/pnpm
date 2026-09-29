# Website documentation

This directory is the source of truth for the non-blog content published on
[pnpm.io](https://pnpm.io). Change documentation in the same pull request as the
behavior it describes. The website, blog, translations and deployment remain in
[pnpm/pnpm.io](https://github.com/pnpm/pnpm.io).

## Editing

Each `versions/<line>/` contains `docs/`, `sidebars.json`, and any documentation
assets under `static/`. The active lines are `12.x`, `11.x`, and `pnpr`. Older
versions are retained here as well. Update both CLI documentation trees when a
fix affects both versions. The Markdown and sidebars were imported from
pnpm/pnpm.io commit `987689309ca8fcd56f7f41ebf3ab83cff2a062c6`.

Use `/img/...` URLs for assets stored under a line's `static/img/` directory.
The publisher places them under `/docs-assets/<line>/img/` and rewrites the
published Markdown, so different releases cannot overwrite each other's images.
Images shared with the blog or website remain available at their original URLs.

With a sibling checkout of pnpm/pnpm.io containing the sync tooling:

```sh
pnpm --dir ../pnpm.io install --frozen-lockfile
node ../pnpm.io/scripts/sync-docs.mjs . --preview
pnpm --dir ../pnpm.io build
pnpm --dir ../pnpm.io check-urls
```

Adjust the paths to your checkouts. `--preview` replaces the website's generated
documentation copies with all local source versions, including archives. Run it
again after editing sources. It does not publish or advance release state.
The Documentation workflow performs the same build and URL checks for PRs.
It pins the website revision for reproducibility; update that revision when
adopting website tooling changes. Release publication uses the website's current
`main` branch.

## Publication

After the Release workflow succeeds, Sync released documentation verifies the
signed tag and npm publication, then imports only that product's documentation
from the tagged commit. Stable CLI releases update their major version; pnpr
releases, including its alpha releases, update the pnpr section. CLI prereleases
do not update stable documentation.

The sync builds and checks the site, then commits the generated copies to
pnpm/pnpm.io. Its Deploy workflow publishes the website. `docs-sync.json` in
that repository records the release version and both source commits. Older
releases cannot replace newer copies, and rerunning a release cannot undo a
later documentation correction.

A sync failure does not undo package publication. Rerun Sync released
documentation with `release_tag` to retry, without publishing packages again.
Concurrent website changes may cause the final push to fail; rerun the sync to
build against the new website commit. The workflow never force-pushes.

For a correction that should ship before the next package release, branch from
the published release tag, change only `docs/versions/<line>/`, and make the
commit available in pnpm/pnpm. Dispatch the sync with that `release_tag` and the
full `docs_commit` SHA. Also carry the correction into the development branch.
The workflow rejects corrections containing code changes or changes to another
version's documentation. This keeps unreleased features off the public site.

For an archived version, import the sources with `--preview` and submit only
that version's generated changes to the website repository for review.

## Initial rollout

1. Land the pnpm/pnpm.io tooling, builder patch and generated copies first.
   Its docs edit links will point at `docs/versions/` in pnpm/pnpm.
2. Land this directory and the Documentation and Sync released documentation
   workflows in pnpm/pnpm. The sync starts with releases whose tags include the
   documentation sources; older tags cannot provide them.
3. Create a `docs-sync` environment in pnpm/pnpm with `DOCS_SYNC_TOKEN`, a token
   with permission to push content to pnpm/pnpm.io's `main` branch. Configure
   branch rules to allow that identity. Use a GitHub App token or fine-grained
   personal access token; the source repository's `GITHUB_TOKEN` cannot perform
   this cross-repository push and trigger the website deployment.
4. Keep Crowdin uploads in pnpm/pnpm.io. Its existing paths and translation
   identifiers remain unchanged. Run `pnpm crowdin-upload` there after a sync
   when updating translation sources, as before.

When adding a major version, add its source tree here and configure its slot in
pnpm/pnpm.io's `versions.json`, preserving the preceding version's published
copies and sidebars. Update the site's `copy-docs` script and ignored Crowdin
copy at the same time. An unconfigured release line fails synchronization
rather than silently changing the site's default version.
