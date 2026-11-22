# Website documentation

`pnpm11/docs/` is the source of the v11 documentation published on
[pnpm.io](https://pnpm.io). Change documentation in the same pull request as the
behavior it describes. The website, blog, translations, and deployment live in
[pnpm/pnpm.io](https://github.com/pnpm/pnpm.io).

## Editing

The Markdown lives directly under `pnpm11/docs/`, with its `sidebars.json` and
documentation assets under `static/`.

Use `/img/...` URLs for assets stored under `static/img/`.
The publisher places them under `/docs-assets/<line>/img/` and rewrites the
published Markdown, so different releases cannot overwrite each other's images.
Images shared with the blog or website remain available at their original URLs.

With a sibling checkout of pnpm/pnpm.io containing the sync tooling:

```sh
pnpm --dir ../pnpm.io install --frozen-lockfile
node ../pnpm.io/scripts/sync-docs.mjs . --preview 11.x
pnpm --dir ../pnpm.io build
pnpm --dir ../pnpm.io check-urls
```

Adjust the paths to your checkouts. `--preview 11.x` replaces the website's
generated 11.x documentation with the local sources. The other lines keep the
website's committed copies. Run the preview again after editing sources. It does not publish or advance release state.
The Documentation workflow performs the same build and URL checks for PRs.
It pins the website revision for reproducibility.

## Publication

When a maintainer publishes the draft GitHub release of a `v11.x.y` tag, Sync
documentation dispatches itself from `main` for that tag. The sync, its retries,
and manual publication are described in
[`DOCUMENTATION.md` on `main`](https://github.com/pnpm/pnpm/blob/main/DOCUMENTATION.md#publication).
The sync imports the documentation from the tagged commit, so it publishes
`pnpm11/docs/` as this branch has it.

### Release pages

Each stable sync also writes the release's page on the pnpm.io blog,
`blog/releases/<version>.md`. The page is the composed changelog section that
the release PR curated (see the
[release-notes skill](.agents/skills/release-notes/SKILL.md)), read at the
release commit. Its lead paragraph becomes the blog excerpt. The sync never
overwrites an existing page, so fix a published page in pnpm/pnpm.io directly.

### Corrections

For a correction that should ship before the next package release, branch from
the published release tag and change only `pnpm11/docs/`. Make the
commit available in pnpm/pnpm. Dispatch Sync documentation on `main` with that
`release_tag` and the full `docs_commit` SHA. Also carry the correction into
`release/11`. The workflow rejects corrections containing code changes or
changes to another version's documentation.

## Markdown by renderer

Each file kind is rendered by a different tool. Use only what its renderer keeps.

| Files | Renderer | Admonitions | Links |
|---|---|---|---|
| `pnpm11/docs/` | Docusaurus on pnpm.io | `:::note`, `:::tip`, `:::warning`, and the other Docusaurus forms | relative `.md` paths between pages; site-rooted `/img/...` and `/blog/...` paths, which the publisher rewrites |
| `.changeset/*.md` | GitHub release notes and the release blog | none | absolute URLs |
| `AGENTS.md`, `CONTRIBUTING.md`, and the other in-repo guides | GitHub, editors, agent tooling | GitHub alerts (`> [!NOTE]`); `WARNING` and `CAUTION` only for a hazard | relative paths |
| READMEs of published npm packages | npm | none | absolute URLs |

GitHub alerts render only on GitHub. Docusaurus admonitions render only on pnpm.io.
