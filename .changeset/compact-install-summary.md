---
"pnpm": minor
---

The install summary prints an updated dependency on one line, such as `↑ lodash 4.17.20 → 4.18.1`. A downgrade is marked with `↓`. The changed part of the new version is colored the way `pnpm outdated` colors it.

In a workspace, a change made by one project is labeled with that project's package name, such as `in @scope/app`. A change made by several projects shows how many, such as `in 5 projects`.

An install now ends with a short checklist above `Done in`, such as `✓ Supply chain · Deprecations · Peers`. A check that found a problem gets its own line, such as `✗ Deprecations: request@2.88.2 and uuid@3.4.0`. A check that did not run is left out. The deprecation and peer warnings that were printed before the summary are replaced by these lines.

Dependencies with newer versions are named on one line of the checklist. Summary lines no longer end with "(x is available)". "Already up to date" is only printed when the install changed nothing.
