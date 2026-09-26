# Support

## Where to ask

**A question is not a bug.** "How do I configure X?", "why does my lockfile look
like this?", and "is this the right way to set up a monorepo?" belong in
[Discussions Q&A](https://github.com/pnpm/pnpm/discussions/categories/q-a),
where other users can answer too. The issue tracker is for defects and concrete
proposals.

Before opening anything:

1. Check the [documentation](https://pnpm.io).
2. Search the [existing issues](https://github.com/pnpm/pnpm/issues?q=is%3Aissue)
   and include closed ones. Most reports have been filed before, often with a
   workaround in the thread.
3. Reproduce on the latest release: `pnpm install -g pnpm@latest`. A large share
   of reports are already fixed.

If your company is migrating to pnpm and has several issues or requests, add a
post for it in
[the companies discussion](https://github.com/pnpm/pnpm/discussions/3787) and
link your issues from there. That is how issues affecting many people get
identified.

## Writing an issue that gets fixed

A **minimal reproduction** is the single most valuable thing you can provide. A
link to a small repository that fails when someone runs `pnpm install` is worth
more than several paragraphs of description, and issues without one often stall
for months. Strip your reproduction down to the dependencies and configuration
that are actually needed to trigger the bug.

Beyond that, a good report states:

- The exact command you ran, and its full output, including the `ERR_PNPM_*`
  code if there is one.
- What you expected to happen instead.
- Your pnpm version, Node.js version, and operating system.

Write for someone who has never seen your project. "It doesn't work" and "the
install is broken" are not reports anyone can start from.

## Commenting on someone else's issue

- **Add information, not weight.** If you hit the same bug, the useful comment
  is the way your case differs: a different OS, a smaller reproduction, the
  commit where it started. If you have nothing to add, use a 👍 reaction. It is
  counted when issues are prioritized, and it does not email everyone watching
  the thread.
- **Skip "any update?"** Subscribe to the issue instead. The thread is updated
  when there is something to say.
- **Keep the thread about its subject.** If your problem turns out to be a
  different one, open a new issue and link it, rather than growing a thread that
  the next reader has to untangle.
- **Reopening.** If an issue was closed and the behavior is still there, say
  precisely what still reproduces and on which version. That is how a mistaken
  close gets corrected.

## How issues are prioritized

pnpm is maintained by a small number of people, most of them working on it in
their own time. There is no support contract behind the tracker and no service
level to appeal to. What moves an issue forward is a reproduction, a diagnosis,
a well-argued proposal, or a pull request. Issues that many people are
demonstrably hitting are worked on sooner.

Two ways to speed up something you need:

- Open a pull request. [`CONTRIBUTING.md`](../CONTRIBUTING.md) covers setting up
  the repository and getting a change reviewed.
- [Sponsor the project](https://opencollective.com/pnpm), which funds the time
  spent maintaining it.

## Conduct

The [Code of Conduct](../CODE_OF_CONDUCT.md) applies to every issue, pull
request, and discussion. Its section "Communicating in Issues, Pull Requests,
and Discussions" covers what is expected in a thread; this page is the practical
version of the same advice.
