# Releasing

Version tags are created by a maintainer, on their own machine, as **signed
annotated tags**. They are deliberately not created by CI: the tag signature is
the one link in the release chain that does not depend on trusting GitHub's
infrastructure, so the key that makes it must never live in Actions secrets.
See [#13578](https://github.com/pnpm/pnpm/issues/13578).

## Steps

1. Run the **Create release PR** workflow (`create-release-pr.yml`) for the
   target branch. It bumps versions, consumes the pending changesets, and opens
   a `release-pr/<base>` PR.

2. Review and merge that PR.

3. Fetch the merge commit and tag each product whose version changed. Tag names
   must match what `release.yml` expects — `v<version>` for the pnpm CLIs,
   `pnpr@<version>` for pnpr:

   | Product            | Manifest                     | Tag prefix |
   | ------------------ | ---------------------------- | ---------- |
   | pnpm (TypeScript)  | `pnpm11/pnpm/package.json`   | `v`        |
   | pacquet (Rust)     | `pnpm/npm/pnpm/package.json` | `v`        |
   | pnpr               | `pnpr/npm/pnpr/package.json` | `pnpr@`    |

   ```bash
   git fetch origin
   git checkout <merge-commit-sha>

   # Tag only the products whose version actually changed this release.
   git tag -s "v$(jq -r .version pnpm11/pnpm/package.json)" -m "v$(jq -r .version pnpm11/pnpm/package.json)"
   git tag -s "v$(jq -r .version pnpm/npm/pnpm/package.json)" -m "v$(jq -r .version pnpm/npm/pnpm/package.json)"
   git tag -s "pnpr@$(jq -r .version pnpr/npm/pnpr/package.json)" -m "pnpr@$(jq -r .version pnpr/npm/pnpr/package.json)"
   ```

   `-s` is what makes the tag verifiable; a lightweight tag (plain `git tag
   <name>`) has no object to carry a signature, and `git verify-tag` fails on it
   with `cannot verify a non-tag object of type commit`.

4. Verify before pushing — this is the check that would have caught the
   regression in #13578:

   ```bash
   git verify-tag v<version>
   ```

5. Push. Each tag push starts a `release.yml` run for the product it names, and
   several tags pushed together release in parallel:

   ```bash
   git push origin v<version> [pnpr@<version> ...]
   ```

## Reruns and partial releases

`release.yml`'s plan job asks npm whether each product's *gate* package is
already published — the one that product's publish job publishes last (`pnpm`
for both CLIs, `@pnpm/pnpr` for pnpr) — and skips the product entirely if it
is. A release that ran to completion is therefore a no-op on rerun.

When a run fails and the fix is a code change, commit the fix, move the tag to
the new commit and push it again:

```bash
git push origin :refs/tags/v<version>          # drop the remote tag
git tag -d v<version>
git tag -s v<version> -m "v<version>"          # re-sign at the fixed commit
git push origin v<version>
```

Re-creating the tag this way changes the ref, which fires a `push` event and
starts a fresh run. Note the tag must be re-signed — `git tag -f` without `-s`
would quietly replace a signed tag with an unsigned one and reintroduce
[#13578](https://github.com/pnpm/pnpm/issues/13578).

Pushing a tag that is *already* on the remote unchanged is a different matter:
it is a no-op, fires no event, and starts no run. To rerun the same commit
without moving the tag, re-run the failed run from the Actions UI or use the
`workflow_dispatch` trigger.

Either way, because the gate package is published last, a run that failed
partway leaves the gate unpublished, so the plan job picks the product up
again. But the packages that did publish before the failure are immutable on
npm and cannot be republished at the same version — read the failed run's logs
to see how far it got. If the fix has to change something already published,
the release needs a new version rather than a retry.

Moving a tag is only safe while the release is still failing. Once a release
has completed and been announced, the tag is what downstream packagers verify
and pin, so it must never be moved.

## How the signature is enforced

The signed-tag rule is not a convention. `release.yml`'s `verify-release-tag`
job imports the public keys committed under `.github/release-keys/` into a
throwaway keyring and runs `git verify-tag` on the pushed tag. Every publish
path descends from `plan`, and `plan` descends from that job, so a tag that is
lightweight, annotated but unsigned, or signed by any key outside that
directory cannot reach a publish step.

This matters because removing the workflow that used to create tags stops CI
from *making* unsigned tags, but on its own would not stop a compromised
workflow or a token with tag scope from pushing one and triggering a release.

To add or rotate a maintainer, commit their armored public key to
`.github/release-keys/` in a reviewed PR. That directory is the release trust
root: anyone whose key lands there can cut a release.

## Signing setup

A maintainer cutting releases needs a PGP key configured for git and registered
with GitHub:

```bash
git config --global user.signingkey <fingerprint>
git config --global tag.gpgsign true      # sign tags by default
git config --global commit.gpgsign true   # sign commits by default

# Register the public key so GitHub shows commits and tags as Verified.
gpg --armor --export <fingerprint> | gh gpg-key add -
```

The key's user ID must carry the same email as `user.email`, and that address
must be verified on the GitHub account — otherwise the signature is valid but
GitHub still renders it Unverified.

## Known gap

The release commit itself is created by `create-release-pr.yml` and merged
through GitHub, so it carries GitHub's web-flow signature rather than a
maintainer's. The signed tag covers the tree, but the full commit chain is not
yet maintainer-signed end to end.
