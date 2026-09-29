# Website documentation

`pnpm/docs/` and `pnpm11/docs/` are the sources for the v12 and v11
documentation published on [pnpm.io](https://pnpm.io). Change documentation in
the same pull request as the behavior it describes. The website, blog, v10 and
archived documentation, pnpr docs, translations, and deployment remain in
[pnpm/pnpm.io](https://github.com/pnpm/pnpm.io).

## Editing

The Markdown lives directly under `pnpm/docs/` for v12 and `pnpm11/docs/` for
v11. Each directory includes its `sidebars.json` and documentation assets under
`static/`. Update both trees when a fix affects both versions. The sources were
imported from pnpm/pnpm.io commit `987689309ca8fcd56f7f41ebf3ab83cff2a062c6`.

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
documentation copies with the local v11 and v12 sources. Run it again after
editing sources. It does not publish or advance release state.
The Documentation workflow performs the same build and URL checks for PRs.
It pins the website revision for reproducibility; update that revision when
adopting website tooling changes. Release publication uses the website's current
`main` branch.

## Publication

After the Release workflow succeeds, Sync released documentation verifies the
signed tag and npm publication, then imports only that version's documentation
from the tagged commit. Only stable v11 and v12 releases trigger a sync. Other
release lines, pnpr releases, and CLI prereleases do not update these docs.

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
the published release tag, change only the corresponding `pnpm/docs/` or
`pnpm11/docs/` directory, and make the commit available in pnpm/pnpm. Dispatch the sync with that `release_tag` and the
full `docs_commit` SHA. Also carry the correction into the development branch.
The workflow rejects corrections containing code changes or changes to another
version's documentation. This keeps unreleased features off the public site.

## Initial rollout

1. Land the pnpm/pnpm.io tooling, builder patch and generated copies first.
   Its v11 and v12 edit links will point at the corresponding product docs in
   pnpm/pnpm.
2. Land the product docs and the Documentation and Sync released documentation
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

Version names and publication paths are managed by pnpm/pnpm.io. Its
`scripts/docs-sources.mjs` maps `11.x` to `pnpm11/docs` and `12.x` to `pnpm/docs`.
The sources here have no additional version directories. A future v10 migration
can be made separately on `release/10` if needed.
