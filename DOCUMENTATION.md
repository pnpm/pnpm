# Website documentation

`pnpm/docs/`, `pnpm11/docs/`, and `pnpr/docs/` are the sources for the v12, v11,
and registry documentation published on [pnpm.io](https://pnpm.io). Change documentation in
the same pull request as the behavior it describes. The website, blog, v10 and
archived documentation, translations, and deployment remain in
[pnpm/pnpm.io](https://github.com/pnpm/pnpm.io).

## Editing

The Markdown lives directly under `pnpm/docs/` for v12 and `pnpm11/docs/` for
v11, with registry documentation under `pnpr/docs/`. Each directory includes
its `sidebars.json` and documentation assets under `static/`. Update both CLI
trees when a fix affects both versions.

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
documentation copies with the local v11, v12, and pnpr sources. Run it again after
editing sources. It does not publish or advance release state.
The Documentation workflow performs the same build and URL checks for PRs.
It pins the website revision for reproducibility; update that revision when
adopting website tooling changes. Release publication uses the website's current
`main` branch.

## Publication

When a maintainer publishes a draft GitHub release, Sync documentation
dispatches itself from `main` for that tag, so every sync runs the workflow,
scripts, and trusted keys on `main`. The sync verifies the
signed tag and npm publication, then imports only that version's documentation
from the tagged commit. Stable v11 and v12 releases update their
respective CLI documentation. Pnpr releases, including alpha releases, update
only the registry documentation. Other CLI releases do not trigger a sync.

The sync builds and checks the site without the publication credential. A
separate job copies only documentation files into a clean checkout, then commits
the generated copies to
pnpm/pnpm.io. Its Deploy workflow publishes the website. `docs-sync.json` in
that repository records the release version and both source commits. Older
releases cannot replace newer copies, and rerunning a release cannot undo a
later documentation correction.

A sync failure does not undo package publication. Rerun Sync documentation
with `release_tag` to retry, without publishing packages again. Release retries
use the currently trusted keys in `.github/release-keys/`; removing a key also
disables retries for tags signed with it. Historical keys are not automatically
re-trusted. Use the manual publication from `main` described below if needed.
Concurrent website changes may cause the final push to fail; rerun the sync to
build against the new website commit. The workflow never force-pushes.

To publish directly from `main` without a release, open **Actions → Sync
documentation → Run workflow**, select `main`, and leave both inputs empty.
This builds, checks, and publishes every product's current documentation,
including docs for unreleased changes. It requires the same `DOCS_SYNC_TOKEN`
setup as automatic publication. The generated website commit records the source
SHA. This manual override leaves the last-release tracking in `docs-sync.json`
unchanged; subsequent newer releases resume updating their respective docs.

### Release pages

Each stable v11 and v12 sync also writes the release's page on the pnpm.io blog,
`blog/releases/<version>.md`. The page is the composed changelog section that
the release PR curated (see the
[release-notes skill](.agents/skills/release-notes/SKILL.md)), read at the
release commit. Its lead paragraph becomes the blog excerpt. The sync never
overwrites an existing page, so fix a published page in pnpm/pnpm.io directly.
Pnpr releases get no page.

### Corrections

For a correction that should ship before the next package release, branch from
the published release tag, change only the corresponding `pnpm/docs/` or
`pnpm11/docs/` directory, or `pnpr/docs/` for registry documentation. Make the
commit available in pnpm/pnpm. Dispatch the sync with that `release_tag` and the
full `docs_commit` SHA. Also carry the correction into the development branch.
The workflow rejects corrections containing code changes or changes to another
version's documentation. This keeps unreleased features off the public site.

## Markdown by renderer

Each file kind is rendered by a different tool. Use only what its renderer keeps.

| Files | Renderer | Admonitions | Links |
|---|---|---|---|
| `pnpm/docs/`, `pnpm11/docs/`, `pnpr/docs/` | Docusaurus on pnpm.io | `:::note`, `:::tip`, `:::warning`, and the other Docusaurus forms | relative `.md` paths between pages; site-rooted `/img/...` and `/blog/...` paths, which the publisher rewrites |
| `.changeset/*.md` | GitHub release notes and the release blog | none | absolute URLs |
| `AGENTS.md`, `CONTRIBUTING.md`, and the other in-repo guides | GitHub, editors, agent tooling | GitHub alerts (`> [!NOTE]`); `WARNING` and `CAUTION` only for a hazard | relative paths |
| READMEs of published npm packages | npm | none | absolute URLs |
| Rust doc comments | rustdoc | none | intra-doc links |
| Doc comments on clap commands and arguments | plain text in `--help` | none | none |

GitHub alerts render only on GitHub. Docusaurus admonitions render only on pnpm.io.

## Initial rollout

1. Land the pnpm/pnpm.io tooling, builder patch and generated copies first.
   Its v11, v12, and pnpr edit links will point at the corresponding product docs in
   pnpm/pnpm.
2. Land the product docs and the Documentation and Sync documentation
   workflows in pnpm/pnpm. The sync starts with releases whose tags include the
   documentation sources; older tags cannot provide them.
3. Create a `docs-sync` environment in pnpm/pnpm with `DOCS_SYNC_TOKEN`, a token
   with permission to push content to pnpm/pnpm.io's `main` branch. Configure
   branch rules to allow that identity. Use a GitHub App token or fine-grained
   personal access token; the source repository's `GITHUB_TOKEN` cannot perform
   this cross-repository push and trigger the website deployment. Limit the
   environment's deployment branches to `main`, so a workflow on another branch
   or a tag cannot read the token.
4. Keep Crowdin uploads in pnpm/pnpm.io. Its existing paths and translation
   identifiers remain unchanged. Run `pnpm crowdin-upload` there after a sync
   when updating translation sources, as before.

Version names and publication paths are managed by pnpm/pnpm.io. Its
`scripts/docs-sources.mjs` maps `11.x` to `pnpm11/docs`, `12.x` to `pnpm/docs`,
and `pnpr` to `pnpr/docs`.
The sources here have no additional version directories. A future v10 migration
can be made separately on `release/10` if needed.
