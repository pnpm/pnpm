## 12.12.0

### Minor Changes

- The install options accept `addMissingPeerTypes`, which applies the `addMissingPeerTypes` setting to the install.

### Patch Changes

- A package set to `false` in `allowBuilds` no longer runs its build scripts when `dangerouslyAllowAllBuilds` is `true`. The two settings together now allow every build except the denied ones. Previously, `dangerouslyAllowAllBuilds` ignored the denials.

- Repeat installs no longer resolve again when a workspace project's `dependencyManifest` differs from its `manifest`. An injected instance of the project is checked against its `dependencyManifest`. Its `workspace:` dependencies are checked against the manifests passed in memory, so project directories need no `package.json`.

  Repeat installs also no longer resolve again when an injected workspace package lists a dependency also as a peer dependency.
