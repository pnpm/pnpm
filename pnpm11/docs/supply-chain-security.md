---
id: supply-chain-security
title: Mitigating supply chain attacks
---

Sometimes npm packages are compromised and published with malware. Luckily, there are companies like [Socket], [Snyk], [Xygeni] and [Aikido] that detect these compromised packages early. The npm registry usually removes the affected versions within hours. However, there is always a window of time between when the malware is published and when it is detected, during which you could be exposed. Fortunately, there are some things you can do with pnpm to minimize the risks.

### Block risky postinstall scripts

Historically, most compromised packages have used `postinstall` scripts to run code immediately upon installation. To mitigate this, pnpm v10 disables the automatic execution of `postinstall` scripts in dependencies. Although there is a setting to re-enable them globally using [dangerouslyAllowAllBuilds], we recommend explicitly listing only trusted dependencies using [allowBuilds]. This way, if a dependency did not require a build in the past, it won't suddenly run a malicious script if a compromised version is published. Still, we recommend being cautious when updating a trusted package that has a `postinstall` script, as [it might get compromised].

### Prevent exotic transitive dependencies

You can prevent transitive dependencies from using exotic sources (like git repositories or direct tarball URLs) by setting [blockExoticSubdeps] to `true`. This ensures that all transitive dependencies are resolved from trusted sources, reducing the risk of supply chain attacks.

### Delay dependency updates

Another way to reduce the risk of installing compromised packages is to delay updates to your dependencies. Since malware is usually detected quickly, delaying updates by 24 hours will most likely prevent you from installing a bad version. The [minimumReleaseAge] setting defines the minimum number of minutes that must pass after a version is published before pnpm will install it. In pnpm v11, this defaults to `1440` (1 day), meaning newly published packages will not be resolved until they are at least 1 day old. To opt out, set `minimumReleaseAge: 0` in `pnpm-workspace.yaml`. You can also set it to `10080` to wait one week before installing a new version.

### Enforce trust with trustPolicy

To further protect your supply chain, pnpm also supports a [trustPolicy] setting. When set to `no-downgrade`, this setting will prevent installation of a package if its trust level has decreased compared to previous releases (for example, if it was previously published by a trusted publisher but now only has provenance or no trust evidence). This helps you avoid installing potentially compromised or less trustworthy versions.

If you need to allow specific packages or versions to bypass the trust policy check, you can use the [trustPolicyExclude] setting. This is useful for known packages that may not meet the trust requirements but are still safe to use.

Additionally, the [trustPolicyIgnoreAfter] setting allows you to ignore trust checks for packages published more than a specified time ago. This is helpful for older versions of packages that lack a process for publishing with signatures or provenance.

### Use a lockfile

It goes without saying that you should always lock your dependencies with a lockfile. Commit your lockfile to your repository to avoid unexpected updates.

If you scan `pnpm-lock.yaml` with a vulnerability scanner or an SBOM generator, check that it handles the [two-document lockfile](lockfile.md): a tool that reads only the first document reports that the project has no dependencies, and no vulnerabilities, without failing.

### Pin dependencies to the registry they come from

If you install from more than one registry, use [namedRegistries] aliases for the packages that must come from a specific one. Since v11.20.0, pnpm records those packages in the lockfile under registry-qualified keys (`<name>@<registryName>:<version>`), so a package cannot be quietly substituted by another registry that publishes the same name and version. See [Named registries in the lockfile]. Packages installed without an alias are resolved through the default [registries] configuration as before.

[Socket]: https://socket.dev/
[Snyk]: https://snyk.io
[Xygeni]: https://xygeni.io/
[Aikido]: https://www.aikido.dev/
[dangerouslyAllowAllBuilds]: settings/build.md#dangerouslyallowallbuilds
[it might get compromised]: https://socket.dev/blog/nx-packages-compromised
[minimumReleaseAge]: settings/dependency-resolution.md#minimumreleaseage
[trustPolicy]: settings/dependency-resolution.md#trustpolicy
[trustPolicyExclude]: settings/dependency-resolution.md#trustpolicyexclude
[allowBuilds]: settings/build.md#allowbuilds
[blockExoticSubdeps]: settings/dependency-resolution.md#blockexoticsubdeps
[trustPolicyIgnoreAfter]: settings/dependency-resolution.md#trustpolicyignoreafter
[namedRegistries]: settings/dependency-resolution.md#namedregistries
[registries]: settings/dependency-resolution.md#registries
[Named registries in the lockfile]: settings/dependency-resolution.md#named-registries-in-the-lockfile
