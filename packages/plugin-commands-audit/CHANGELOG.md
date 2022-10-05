# @pnpm/plugin-commands-audit

## 6.1.23

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/audit@3.1.10
  - @pnpm/cli-utils@0.7.41
  - @pnpm/config@15.10.10
  - @pnpm/lockfile-file@5.3.7
  - @pnpm/read-project-manifest@3.0.12

## 6.1.22

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/config@15.10.9
  - @pnpm/cli-utils@0.7.40
  - @pnpm/audit@3.1.9
  - @pnpm/lockfile-file@5.3.6
  - @pnpm/read-project-manifest@3.0.11

## 6.1.21

### Patch Changes

- @pnpm/config@15.10.8
- @pnpm/cli-utils@0.7.39

## 6.1.20

### Patch Changes

- @pnpm/config@15.10.7
- @pnpm/cli-utils@0.7.38

## 6.1.19

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/audit@3.1.8
  - @pnpm/cli-utils@0.7.37
  - @pnpm/config@15.10.6
  - @pnpm/lockfile-file@5.3.5
  - @pnpm/read-project-manifest@3.0.10

## 6.1.18

### Patch Changes

- @pnpm/config@15.10.5
- @pnpm/cli-utils@0.7.36

## 6.1.17

### Patch Changes

- @pnpm/cli-utils@0.7.35
- @pnpm/config@15.10.4

## 6.1.16

### Patch Changes

- Updated dependencies [0373af22e]
  - @pnpm/lockfile-file@5.3.4
  - @pnpm/audit@3.1.7
  - @pnpm/config@15.10.3
  - @pnpm/cli-utils@0.7.34

## 6.1.15

### Patch Changes

- @pnpm/config@15.10.2
- @pnpm/cli-utils@0.7.33

## 6.1.14

### Patch Changes

- @pnpm/config@15.10.1
- @pnpm/cli-utils@0.7.32

## 6.1.13

### Patch Changes

- a12d1a011: - Add new Error type: AuditEndpointNotExistsError
  - On AuditUrl returns 404, AuditEndpointNotExistsError will throw
  - When audit handler catches AuditEndpointNotExistsError, the command will return to avoid execute further codes
- Updated dependencies [2aa22e4b1]
- Updated dependencies [a12d1a011]
  - @pnpm/config@15.10.0
  - @pnpm/audit@3.1.7
  - @pnpm/cli-utils@0.7.31

## 6.1.12

### Patch Changes

- @pnpm/config@15.9.4
- @pnpm/cli-utils@0.7.30

## 6.1.11

### Patch Changes

- @pnpm/config@15.9.3
- @pnpm/cli-utils@0.7.29

## 6.1.10

### Patch Changes

- @pnpm/config@15.9.2
- @pnpm/cli-utils@0.7.28

## 6.1.9

### Patch Changes

- @pnpm/config@15.9.1
- @pnpm/cli-utils@0.7.27
- @pnpm/audit@3.1.6

## 6.1.8

### Patch Changes

- Updated dependencies [1e5482da4]
  - @pnpm/lockfile-file@5.3.3
  - @pnpm/audit@3.1.5
  - @pnpm/config@15.9.0

## 6.1.7

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
  - @pnpm/read-project-manifest@3.0.9
  - @pnpm/config@15.9.0
  - @pnpm/lockfile-file@5.3.2
  - @pnpm/cli-utils@0.7.26
  - @pnpm/audit@3.1.5

## 6.1.6

### Patch Changes

- Updated dependencies [44544b493]
- Updated dependencies [c90798461]
- Updated dependencies [34121d753]
  - @pnpm/lockfile-file@5.3.1
  - @pnpm/types@8.5.0
  - @pnpm/config@15.8.1
  - @pnpm/audit@3.1.4
  - @pnpm/cli-utils@0.7.25
  - @pnpm/read-project-manifest@3.0.8

## 6.1.5

### Patch Changes

- @pnpm/audit@3.1.3

## 6.1.4

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0
  - @pnpm/cli-utils@0.7.24

## 6.1.3

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-file@5.3.0
  - @pnpm/audit@3.1.2
  - @pnpm/config@15.7.1
  - @pnpm/cli-utils@0.7.23

## 6.1.2

### Patch Changes

- Updated dependencies [01c5834bf]
- Updated dependencies [4fa1091c8]
  - @pnpm/read-project-manifest@3.0.7
  - @pnpm/config@15.7.0
  - @pnpm/lockfile-file@5.2.0
  - @pnpm/cli-utils@0.7.22
  - @pnpm/audit@3.1.1

## 6.1.1

### Patch Changes

- Updated dependencies [7334b347b]
  - @pnpm/config@15.6.1
  - @pnpm/cli-utils@0.7.21
  - @pnpm/audit@3.1.1

## 6.1.0

### Minor Changes

- af79b6184: Add authentication to audit command

### Patch Changes

- Updated dependencies [af79b6184]
- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/audit@3.1.0
  - @pnpm/config@15.6.0
  - @pnpm/cli-utils@0.7.20

## 6.0.21

### Patch Changes

- @pnpm/config@15.5.2
- @pnpm/cli-utils@0.7.19

## 6.0.20

### Patch Changes

- @pnpm/cli-utils@0.7.18
- @pnpm/audit@3.0.10

## 6.0.19

### Patch Changes

- Updated dependencies [ab684d77e]
  - @pnpm/lockfile-file@5.1.4
  - @pnpm/audit@3.0.9

## 6.0.18

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/cli-utils@0.7.17
  - @pnpm/config@15.5.1
  - @pnpm/lockfile-file@5.1.3
  - @pnpm/audit@3.0.9

## 6.0.17

### Patch Changes

- d3ad3368f: `pnpm audit --fix` should not add an override for a vulnerable package that has no fixes released.
  - @pnpm/audit@3.0.8

## 6.0.16

### Patch Changes

- Updated dependencies [f48d46ef6]
  - @pnpm/config@15.5.0
  - @pnpm/cli-utils@0.7.16

## 6.0.15

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/audit@3.0.7
  - @pnpm/lockfile-file@5.1.2
  - @pnpm/cli-utils@0.7.15
  - @pnpm/config@15.4.1
  - @pnpm/read-project-manifest@3.0.6

## 6.0.14

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/types@8.3.0
  - @pnpm/config@15.4.0
  - @pnpm/audit@3.0.6
  - @pnpm/cli-utils@0.7.14
  - @pnpm/lockfile-file@5.1.1
  - @pnpm/read-project-manifest@3.0.5

## 6.0.13

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
  - @pnpm/types@8.2.0
  - @pnpm/config@15.3.0
  - @pnpm/lockfile-file@5.1.0
  - @pnpm/audit@3.0.5
  - @pnpm/cli-utils@0.7.13
  - @pnpm/read-project-manifest@3.0.4

## 6.0.12

### Patch Changes

- Updated dependencies [25798aad1]
  - @pnpm/config@15.2.1
  - @pnpm/cli-utils@0.7.12

## 6.0.11

### Patch Changes

- Updated dependencies [4d39e4a0c]
- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
  - @pnpm/types@8.1.0
  - @pnpm/config@15.2.0
  - @pnpm/audit@3.0.4
  - @pnpm/cli-utils@0.7.11
  - @pnpm/lockfile-file@5.0.4
  - @pnpm/read-project-manifest@3.0.3

## 6.0.10

### Patch Changes

- @pnpm/cli-utils@0.7.10
- @pnpm/config@15.1.4

## 6.0.9

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4
  - @pnpm/cli-utils@0.7.9

## 6.0.8

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3
  - @pnpm/cli-utils@0.7.8

## 6.0.7

### Patch Changes

- @pnpm/cli-utils@0.7.7

## 6.0.6

### Patch Changes

- Updated dependencies [af22c6c4f]
  - @pnpm/config@15.1.2
  - @pnpm/cli-utils@0.7.6
  - @pnpm/audit@3.0.3

## 6.0.5

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/cli-utils@0.7.5
  - @pnpm/lockfile-file@5.0.3
  - @pnpm/audit@3.0.2

## 6.0.4

### Patch Changes

- @pnpm/cli-utils@0.7.4
- @pnpm/config@15.1.1

## 6.0.3

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/audit@3.0.2
  - @pnpm/cli-utils@0.7.3
  - @pnpm/config@15.1.1
  - @pnpm/lockfile-file@5.0.2
  - @pnpm/read-project-manifest@3.0.2

## 6.0.2

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0
  - @pnpm/cli-utils@0.7.2

## 6.0.1

### Patch Changes

- Updated dependencies [8dac029ef]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
- Updated dependencies [1267e4eff]
  - @pnpm/config@15.0.0
  - @pnpm/constants@6.1.0
  - @pnpm/cli-utils@0.7.1
  - @pnpm/audit@3.0.1
  - @pnpm/error@3.0.1
  - @pnpm/lockfile-file@5.0.1
  - @pnpm/read-project-manifest@3.0.1

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/types@8.0.0
  - @pnpm/audit@3.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/error@3.0.0
  - @pnpm/lockfile-file@5.0.0
  - @pnpm/read-project-manifest@3.0.0
  - @pnpm/cli-utils@0.7.0

## 5.1.42

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/audit@2.2.7
  - @pnpm/cli-utils@0.6.50
  - @pnpm/config@13.13.2
  - @pnpm/lockfile-file@4.3.1
  - @pnpm/read-project-manifest@2.0.13

## 5.1.41

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-file@4.3.0
  - @pnpm/types@7.10.0
  - @pnpm/audit@2.2.6
  - @pnpm/cli-utils@0.6.49
  - @pnpm/config@13.13.1
  - @pnpm/read-project-manifest@2.0.12

## 5.1.40

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0
  - @pnpm/cli-utils@0.6.48

## 5.1.39

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0
  - @pnpm/cli-utils@0.6.47

## 5.1.38

### Patch Changes

- @pnpm/audit@2.2.5
- @pnpm/config@13.11.0

## 5.1.37

### Patch Changes

- Updated dependencies [fff0e4493]
  - @pnpm/config@13.11.0
  - @pnpm/cli-utils@0.6.46

## 5.1.36

### Patch Changes

- @pnpm/cli-utils@0.6.45

## 5.1.35

### Patch Changes

- Updated dependencies [e76151f66]
- Updated dependencies [26cd01b88]
  - @pnpm/config@13.10.0
  - @pnpm/types@7.9.0
  - @pnpm/cli-utils@0.6.44
  - @pnpm/audit@2.2.4
  - @pnpm/lockfile-file@4.2.6
  - @pnpm/read-project-manifest@2.0.11

## 5.1.34

### Patch Changes

- @pnpm/cli-utils@0.6.43

## 5.1.33

### Patch Changes

- Updated dependencies [8fe8f5e55]
  - @pnpm/config@13.9.0
  - @pnpm/cli-utils@0.6.42

## 5.1.32

### Patch Changes

- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/config@13.8.0
  - @pnpm/cli-utils@0.6.41

## 5.1.31

### Patch Changes

- @pnpm/audit@2.2.3
- @pnpm/cli-utils@0.6.40
- @pnpm/config@13.7.2
- @pnpm/lockfile-file@4.2.5
- @pnpm/read-project-manifest@2.0.10

## 5.1.30

### Patch Changes

- Updated dependencies [eb9ebd0f3]
- Updated dependencies [eb9ebd0f3]
  - @pnpm/lockfile-file@4.2.4
  - @pnpm/audit@2.2.2

## 5.1.29

### Patch Changes

- @pnpm/cli-utils@0.6.39

## 5.1.28

### Patch Changes

- @pnpm/audit@2.2.2
- @pnpm/cli-utils@0.6.38
- @pnpm/config@13.7.1
- @pnpm/lockfile-file@4.2.3
- @pnpm/read-project-manifest@2.0.9

## 5.1.27

### Patch Changes

- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
  - @pnpm/config@13.7.0
  - @pnpm/cli-utils@0.6.37
  - @pnpm/audit@2.2.1
  - @pnpm/lockfile-file@4.2.2
  - @pnpm/read-project-manifest@2.0.8

## 5.1.26

### Patch Changes

- Updated dependencies [f1c194ded]
- Updated dependencies [46aaf7108]
  - @pnpm/audit@2.2.0
  - @pnpm/config@13.6.1
  - @pnpm/cli-utils@0.6.36

## 5.1.25

### Patch Changes

- Updated dependencies [8a99a01ff]
  - @pnpm/config@13.6.0
  - @pnpm/audit@2.1.15
  - @pnpm/cli-utils@0.6.35

## 5.1.24

### Patch Changes

- @pnpm/cli-utils@0.6.34

## 5.1.23

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1
  - @pnpm/cli-utils@0.6.33

## 5.1.22

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/config@13.5.0
  - @pnpm/cli-utils@0.6.32
  - @pnpm/audit@2.1.14

## 5.1.21

### Patch Changes

- @pnpm/cli-utils@0.6.31

## 5.1.20

### Patch Changes

- @pnpm/config@13.4.2
- @pnpm/audit@2.1.13
- @pnpm/cli-utils@0.6.30
- @pnpm/lockfile-file@4.2.1
- @pnpm/read-project-manifest@2.0.7

## 5.1.19

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/lockfile-file@4.2.0
  - @pnpm/audit@2.1.12
  - @pnpm/cli-utils@0.6.29
  - @pnpm/config@13.4.1
  - @pnpm/read-project-manifest@2.0.6

## 5.1.18

### Patch Changes

- @pnpm/audit@2.1.11

## 5.1.17

### Patch Changes

- Updated dependencies [b6d74c545]
  - @pnpm/config@13.4.0
  - @pnpm/cli-utils@0.6.28

## 5.1.16

### Patch Changes

- Updated dependencies [bd7bcdbe8]
  - @pnpm/config@13.3.0
  - @pnpm/cli-utils@0.6.27
  - @pnpm/audit@2.1.10

## 5.1.15

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0
  - @pnpm/cli-utils@0.6.26

## 5.1.14

### Patch Changes

- @pnpm/cli-utils@0.6.25

## 5.1.13

### Patch Changes

- Updated dependencies [4027a3c69]
  - @pnpm/config@13.1.0
  - @pnpm/cli-utils@0.6.24

## 5.1.12

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0
  - @pnpm/cli-utils@0.6.23

## 5.1.11

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0
  - @pnpm/cli-utils@0.6.22

## 5.1.10

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0
  - @pnpm/cli-utils@0.6.21
  - @pnpm/audit@2.1.9

## 5.1.9

### Patch Changes

- @pnpm/audit@2.1.8
- @pnpm/cli-utils@0.6.20

## 5.1.8

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19

## 5.1.7

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18

## 5.1.6

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17

## 5.1.5

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16

## 5.1.4

### Patch Changes

- 92ed1272e: If a package has no fixes, do not add it to the overrides.
- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/cli-utils@0.6.15

## 5.1.3

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/cli-utils@0.6.14

## 5.1.2

### Patch Changes

- @pnpm/audit@2.1.7
- @pnpm/cli-utils@0.6.13
- @pnpm/config@12.4.3
- @pnpm/lockfile-file@4.1.1
- @pnpm/read-project-manifest@2.0.5

## 5.1.1

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2
  - @pnpm/cli-utils@0.6.12

## 5.1.0

### Minor Changes

- a5f698290: New command added: `pnpm audit --fix`. This command adds overrides to `package.json` that force versions of packages that do not have the vulnerabilities.

### Patch Changes

- @pnpm/cli-utils@0.6.11

## 5.0.1

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1
  - @pnpm/cli-utils@0.6.10

## 5.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/cli-utils@0.6.9

## 4.2.2

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/lockfile-file@4.1.0
  - @pnpm/audit@2.1.6
  - @pnpm/cli-utils@0.6.8
  - @pnpm/config@12.3.3

## 4.2.1

### Patch Changes

- @pnpm/audit@2.1.5

## 4.2.0

### Minor Changes

- 448710f88: New CLI option added: `--ignore-registry-errors`. When used, audit exits with 0 exit code, when the registry responds with a non-200 status code.

### Patch Changes

- Updated dependencies [2dc5a7a4c]
  - @pnpm/lockfile-file@4.0.4
  - @pnpm/audit@2.1.4

## 4.1.6

### Patch Changes

- @pnpm/audit@2.1.4
- @pnpm/cli-utils@0.6.7
- @pnpm/config@12.3.2
- @pnpm/lockfile-file@4.0.3

## 4.1.5

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/lockfile-file@4.0.2
  - @pnpm/cli-utils@0.6.6
  - @pnpm/audit@2.1.3

## 4.1.4

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0
  - @pnpm/cli-utils@0.6.5

## 4.1.3

### Patch Changes

- @pnpm/cli-utils@0.6.4
- @pnpm/audit@2.1.2

## 4.1.2

### Patch Changes

- @pnpm/cli-utils@0.6.3
- @pnpm/config@12.2.0

## 4.1.1

### Patch Changes

- Updated dependencies [40b75fbb9]
  - @pnpm/audit@2.1.1
  - @pnpm/config@12.2.0

## 4.1.0

### Minor Changes

- 05baaa6e7: Add new config setting: `fetch-timeout`.

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [05baaa6e7]
  - @pnpm/config@12.2.0
  - @pnpm/audit@2.1.0
  - @pnpm/cli-utils@0.6.2
  - @pnpm/lockfile-file@4.0.1

## 4.0.2

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0
  - @pnpm/cli-utils@0.6.1

## 4.0.1

### Patch Changes

- @pnpm/audit@2.0.1

## 4.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [155e70597]
- Updated dependencies [9c2a878c3]
- Updated dependencies [aed712455]
- Updated dependencies [8b66f26dc]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f7750baed]
- Updated dependencies [aed712455]
- Updated dependencies [9c2a878c3]
  - @pnpm/constants@5.0.0
  - @pnpm/audit@2.0.0
  - @pnpm/cli-utils@0.6.0
  - @pnpm/config@12.0.0
  - @pnpm/error@2.0.0
  - @pnpm/lockfile-file@4.0.0

## 3.0.6

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2
  - @pnpm/cli-utils@0.5.4

## 3.0.5

### Patch Changes

- Updated dependencies [4b3852c39]
  - @pnpm/config@11.14.1
  - @pnpm/cli-utils@0.5.3

## 3.0.4

### Patch Changes

- @pnpm/audit@1.1.24

## 3.0.3

### Patch Changes

- @pnpm/config@11.14.0
- @pnpm/cli-utils@0.5.2

## 3.0.2

### Patch Changes

- Updated dependencies [3be2b1773]
  - @pnpm/cli-utils@0.5.1

## 3.0.1

### Patch Changes

- Updated dependencies [51e1456dd]
  - @pnpm/lockfile-file@3.2.1
  - @pnpm/audit@1.1.23

## 3.0.0

### Major Changes

- 5175460a0: Filter dependency types via the `dev`/`production`/`optional` options instead of the `included` option.

## 2.0.43

### Patch Changes

- 0c11e1a07: Audit output should always have a new line at the end.
- Updated dependencies [cb040ae18]
  - @pnpm/cli-utils@0.5.0
  - @pnpm/config@11.14.0

## 2.0.42

### Patch Changes

- Updated dependencies [c4cc62506]
  - @pnpm/config@11.13.0
  - @pnpm/cli-utils@0.4.51

## 2.0.41

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1
  - @pnpm/cli-utils@0.4.50

## 2.0.40

### Patch Changes

- @pnpm/cli-utils@0.4.49

## 2.0.39

### Patch Changes

- @pnpm/cli-utils@0.4.48

## 2.0.38

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
- Updated dependencies [548f28df9]
  - @pnpm/lockfile-file@3.2.0
  - @pnpm/cli-utils@0.4.47
  - @pnpm/config@11.12.0
  - @pnpm/audit@1.1.23

## 2.0.37

### Patch Changes

- @pnpm/config@11.11.1
- @pnpm/cli-utils@0.4.46

## 2.0.36

### Patch Changes

- Updated dependencies [af897c324]
  - @pnpm/lockfile-file@3.1.4
  - @pnpm/audit@1.1.22

## 2.0.35

### Patch Changes

- Updated dependencies [1e4a3a17a]
- Updated dependencies [f40bc5927]
  - @pnpm/lockfile-file@3.1.3
  - @pnpm/config@11.11.0
  - @pnpm/audit@1.1.22
  - @pnpm/cli-utils@0.4.45

## 2.0.34

### Patch Changes

- Updated dependencies [425c7547d]
  - @pnpm/config@11.10.2
  - @pnpm/cli-utils@0.4.44
  - @pnpm/audit@1.1.22

## 2.0.33

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1
  - @pnpm/cli-utils@0.4.43

## 2.0.32

### Patch Changes

- Updated dependencies [a8656b42f]
  - @pnpm/config@11.10.0
  - @pnpm/cli-utils@0.4.42

## 2.0.31

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1
  - @pnpm/cli-utils@0.4.41

## 2.0.30

### Patch Changes

- Updated dependencies [fba715512]
  - @pnpm/lockfile-file@3.1.2
  - @pnpm/audit@1.1.21

## 2.0.29

### Patch Changes

- @pnpm/audit@1.1.20

## 2.0.28

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/cli-utils@0.4.40
  - @pnpm/audit@1.1.19

## 2.0.27

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0
  - @pnpm/cli-utils@0.4.39

## 2.0.26

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/audit@1.1.18
  - @pnpm/cli-utils@0.4.38
  - @pnpm/config@11.7.2
  - @pnpm/lockfile-file@3.1.1

## 2.0.25

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0
  - @pnpm/audit@1.1.17

## 2.0.24

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/audit@1.1.17
  - @pnpm/cli-utils@0.4.37

## 2.0.23

### Patch Changes

- e70232907: Use @arcanis/slice-ansi instead of slice-ansi.
- Updated dependencies [aa6bc4f95]
  - @pnpm/lockfile-file@3.0.17
  - @pnpm/audit@1.1.17
  - @pnpm/cli-utils@0.4.36

## 2.0.22

### Patch Changes

- @pnpm/audit@1.1.16
- @pnpm/lockfile-file@3.0.16
- @pnpm/cli-utils@0.4.35
- @pnpm/config@11.7.1

## 2.0.21

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0
  - @pnpm/cli-utils@0.4.34

## 2.0.20

### Patch Changes

- Updated dependencies [fcdad632f]
  - @pnpm/constants@4.1.0
  - @pnpm/audit@1.1.15
  - @pnpm/lockfile-file@3.0.15
  - @pnpm/cli-utils@0.4.33
  - @pnpm/config@11.6.1

## 2.0.19

### Patch Changes

- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0
  - @pnpm/cli-utils@0.4.32

## 2.0.18

### Patch Changes

- @pnpm/cli-utils@0.4.31

## 2.0.17

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0
  - @pnpm/cli-utils@0.4.30

## 2.0.16

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0
  - @pnpm/cli-utils@0.4.29

## 2.0.15

### Patch Changes

- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0
  - @pnpm/cli-utils@0.4.28

## 2.0.14

### Patch Changes

- @pnpm/audit@1.1.14
- @pnpm/cli-utils@0.4.27

## 2.0.13

### Patch Changes

- @pnpm/audit@1.1.13

## 2.0.12

### Patch Changes

- @pnpm/cli-utils@0.4.26

## 2.0.11

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7
  - @pnpm/audit@1.1.12
  - @pnpm/cli-utils@0.4.25
  - @pnpm/lockfile-file@3.0.14

## 2.0.10

### Patch Changes

- 6138b56d0: Update table to v6.

## 2.0.9

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6
  - @pnpm/cli-utils@0.4.24

## 2.0.8

### Patch Changes

- Updated dependencies [9550b0505]
- Updated dependencies [972864e0d]
  - @pnpm/lockfile-file@3.0.13
  - @pnpm/config@11.2.5
  - @pnpm/audit@1.1.11
  - @pnpm/cli-utils@0.4.23

## 2.0.7

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/audit@1.1.11
  - @pnpm/cli-utils@0.4.22
  - @pnpm/config@11.2.4
  - @pnpm/lockfile-file@3.0.12

## 2.0.6

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3
  - @pnpm/cli-utils@0.4.21

## 2.0.5

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2
  - @pnpm/cli-utils@0.4.20

## 2.0.4

### Patch Changes

- @pnpm/cli-utils@0.4.19

## 2.0.3

### Patch Changes

- @pnpm/cli-utils@0.4.18

## 2.0.2

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/config@11.2.1
  - @pnpm/audit@1.1.10
  - @pnpm/cli-utils@0.4.17

## 2.0.1

### Patch Changes

- 8bb015059: `pnpm audit --audit-level high` should not error if the found vulnerabilities are low and/or moderate.

## 2.0.0

### Major Changes

- a64b7250c: Return `Promise&lt;{ output: string, exitCode: number }>` instead of `Promise&lt;string>`.

  `exitCode` is `1` when there are any packages with vulnerabilities in the dependencies.

## 1.0.21

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/cli-utils@0.4.16
  - @pnpm/config@11.2.0

## 1.0.20

### Patch Changes

- 4e5e22aab: Allow to set a custom registry through the `--registry` option, when running `pnpm audit` (#2689).

## 1.0.19

### Patch Changes

- @pnpm/audit@1.1.9
- @pnpm/cli-utils@0.4.15

## 1.0.18

### Patch Changes

- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/config@11.1.0
  - @pnpm/cli-utils@0.4.14
  - @pnpm/audit@1.1.8

## 1.0.17

### Patch Changes

- @pnpm/config@11.0.1
- @pnpm/cli-utils@0.4.13

## 1.0.16

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0
  - @pnpm/cli-utils@0.4.12
  - @pnpm/audit@1.1.7

## 1.0.15

### Patch Changes

- @pnpm/config@10.0.1
- @pnpm/cli-utils@0.4.11

## 1.0.14

### Patch Changes

- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
  - @pnpm/config@10.0.0
  - @pnpm/cli-utils@0.4.10
  - @pnpm/audit@1.1.6
  - @pnpm/lockfile-file@3.0.11

## 1.0.13

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/config@9.2.0
  - @pnpm/audit@1.1.5
  - @pnpm/cli-utils@0.4.9
  - @pnpm/lockfile-file@3.0.10

## 1.0.12

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.
- Updated dependencies [e934b1a48]
  - @pnpm/cli-utils@0.4.8
  - @pnpm/audit@1.1.4

## 1.0.11

### Patch Changes

- @pnpm/audit@1.1.3
- @pnpm/cli-utils@0.4.7

## 1.0.10

### Patch Changes

- Updated dependencies [ffddf34a8]
  - @pnpm/config@9.1.0
  - @pnpm/cli-utils@0.4.6

## 1.0.9

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/config@9.0.0
  - @pnpm/audit@1.1.2
  - @pnpm/cli-utils@0.4.5
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-file@3.0.9

## 1.0.9-alpha.2

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/lockfile-file@3.0.9-alpha.2
  - @pnpm/audit@1.1.2-alpha.2

## 1.0.9-alpha.1

### Patch Changes

- @pnpm/audit@1.1.2-alpha.1
- @pnpm/cli-utils@0.4.5-alpha.1
- @pnpm/config@8.3.1-alpha.1
- @pnpm/lockfile-file@3.0.9-alpha.1

## 1.0.9-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.0
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/audit@1.1.1-alpha.0

## 1.0.8

### Patch Changes

- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/audit@1.1.0
  - @pnpm/cli-utils@0.4.4
