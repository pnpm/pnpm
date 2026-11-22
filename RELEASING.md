# Releasing

Version tags are created by a maintainer, on their own machine, as **signed
annotated tags**. They are deliberately not created by CI: the tag signature is
the one link in the release chain that does not depend on trusting GitHub's
infrastructure, so the key that makes it must never live in Actions secrets.
See [#13578](https://github.com/pnpm/pnpm/issues/13578).

This branch releases pnpm v11: `pnpm`, `@pnpm/exe` and its platform packages,
and the v11 library packages.

## Steps

1. Run the **Create Release PR** workflow (`create-release-pr.yml`) from the
   `release/11` branch with target `release/11`. The workflow refuses a target
   that differs from the branch it runs from. It bumps versions, consumes the
   pending changesets, and opens a `release-pr/release/11` PR.

2. Review and merge that PR.

3. Fetch the merge commit and tag it with the version in
   `pnpm11/pnpm/package.json`:

   ```bash
   git fetch origin
   SHA=<merge-commit-sha>          # the commit the release PR merged as
   TAG="v$(git show "$SHA:pnpm11/pnpm/package.json" | jq -r .version)"

   # Always pass the target commit: a bare `git tag -s <name>` signs whatever
   # HEAD happens to be.
   git tag -s "$TAG" -m "$TAG" "$SHA"
   ```

   `-s` is what makes the tag verifiable; a lightweight tag (plain `git tag
   <name>`) has no object to carry a signature, and `git verify-tag` fails on it
   with `cannot verify a non-tag object of type commit`.

4. Verify the tag before pushing. `release.yml` rejects an unsigned tag
   anyway, but catching it here saves a failed run:

   ```bash
   git verify-tag "$TAG"
   ```

5. Push. The tag push starts this branch's `release.yml`:

   ```bash
   git push origin "$TAG"
   ```

6. After the workflow finishes, approve the staged npm packages. The release
   stages `@pnpm/exe` and then `pnpm`. Approve them from a maintainer's machine:

   ```bash
   pnpm stage approve
   ```

   The command lists every staged version and approves the ones selected in the
   picker. It downloads the selected tarballs first and reads their published
   manifests to determine dependency order. It stops before publishing a
   package whose selected dependency failed to be approved. Passing the stage
   IDs from the completed job's summary (`pnpm stage approve <stage-id> ...`)
   approves that set instead.

   Approval requires interactive 2FA, once for the whole selection. pnpm asks
   for another one-time password only when the registry stops accepting the
   one it holds. The npm trusted publishers for `pnpm`, `@pnpm/exe`, and their
   platform packages must allow staged publishing only, so CI can stage a
   release but cannot approve or publish it directly.

7. Publish the draft GitHub release the workflow created. Publishing it starts
   Sync documentation for that version, and the sync fails while the version
   is still staged on npm. So publish the draft only after its packages are
   approved.

The Tag workflow, which moves npm dist-tags, runs from `main`.

## Reruns and partial releases

`release.yml`'s plan job asks npm whether `pnpm`, the package the publish job
publishes last, is already published, and skips the release if it is. A
release that ran to completion is therefore a no-op on rerun.

When a run fails and the fix is a code change, commit the fix, move the tag to
the new commit and push it again:

```bash
TAG=v<version>
FIXED=<sha-of-the-fix-commit>

git push origin ":refs/tags/$TAG"     # drop the remote tag
git tag -d "$TAG"
git tag -s "$TAG" -m "$TAG" "$FIXED"  # re-sign, at the fixed commit explicitly
git verify-tag "$TAG"
git push origin "$TAG"
```

Pass `$FIXED` rather than letting `git tag -s` default to `HEAD`: the tag is
what `release.yml` builds and publishes from, so a tag that accidentally names
the wrong commit ships unreviewed code under a released version number.

Re-creating the tag this way changes the ref, which fires a `push` event and
starts a fresh run. The tag must be re-signed. `git tag -f` without `-s` would
replace a signed tag with an unsigned one, which `release.yml` rejects.

Pushing a tag that is *already* on the remote unchanged is a no-op: it fires no
event and starts no run. To rerun the same commit without moving the tag,
re-run the failed run from the Actions UI, or dispatch the workflow **against
the release tag itself**. `validate-release-ref` rejects a dispatch from a
branch.

Either way, because `pnpm` is published last, a run that failed partway leaves
it unpublished, so the plan job picks the release up again. But the packages
that did publish before the failure are immutable on npm and cannot be
republished at the same version. Read the failed run's logs to see how far it
got. If the fix has to change something already published, the release needs a
new version rather than a retry.

If a run stopped after staging a package but before it became public, first
approve or reject that pending stage from a maintainer's machine. CI cannot
inspect or remove it with its stage-only OIDC permission, and trying to stage
the same package version again will fail. The stage ID remains available in
the stopped run's log and job summary.

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

To add or rotate a maintainer, commit their armored public key to
`.github/release-keys/` in a reviewed PR. That directory is the release trust
root: anyone whose key lands there can cut a release.

## Signing setup

A maintainer cutting releases needs a PGP key configured for git and registered
with GitHub:

<!-- cspell:ignore signingkey -->

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

## Website documentation

Publishing the GitHub release dispatches the Sync documentation workflow on
`main`, which copies the tagged `pnpm11/docs/` and the release page to
pnpm/pnpm.io. Check that workflow as part of finishing a release. A failed docs
sync can be retried independently of package publication. See
[DOCUMENTATION.md](DOCUMENTATION.md#publication).
