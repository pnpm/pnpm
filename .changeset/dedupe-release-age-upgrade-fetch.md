---
"pacquet": patch
---

Fixed a severe slowdown resolving large workspaces against registries whose abbreviated metadata lacks per-version `time` fields (such as `node-registry.bit.cloud`) while `minimumReleaseAge` is active. The resolver upgraded the abbreviated packument to full metadata once per *dependency edge* instead of once per package — re-requesting the same packument from the registry hundreds of times in a single install — and a `304 Not Modified` answer was never remembered, so the round trip repeated forever. The upgrade outcome is now cached for the rest of the install. On a 345-project workspace this cut a full resolution from 105 s to 36 s.

Also stopped the resolver from deep-copying every workspace project manifest on each internal resolve-options clone (the workspace-packages map is now shared by reference).
