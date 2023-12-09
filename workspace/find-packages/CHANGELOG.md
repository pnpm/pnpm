# @pnpm/find-workspace-packages

## 1.1.5

### Patch Changes

- Updated dependencies [e8926e920]
  - @pnpm/workspace.read-manifest@1.0.2
  - @pnpm/cli-utils@2.1.4

## 1.1.4

### Patch Changes

- @pnpm/cli-utils@2.1.3

## 1.1.3

### Patch Changes

- Updated dependencies [e2a0c7272]
  - @pnpm/workspace.read-manifest@1.0.1

## 1.1.2

### Patch Changes

- Updated dependencies [3f7e65e10]
  - @pnpm/workspace.read-manifest@1.0.0
  - @pnpm/cli-utils@2.1.2

## 1.1.1

### Patch Changes

- @pnpm/cli-utils@2.1.1

## 1.1.0

### Minor Changes

- 43ce9e4a6: Support for multiple architectures when installing dependencies [#5965](https://github.com/pnpm/pnpm/issues/5965).

  You can now specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install. Use the `supportedArchitectures` field in `package.json` to define your preferences.

  For example, the following configuration tells pnpm to install optional dependencies for Windows x64:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32"],
        "cpu": ["x64"]
      }
    }
  }
  ```

  Whereas this configuration will have pnpm install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32", "darwin", "current"],
        "cpu": ["x64", "arm64"]
      }
    }
  }
  ```

  Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/cli-utils@2.1.0
  - @pnpm/fs.find-packages@2.0.8

## 1.0.14

### Patch Changes

- @pnpm/cli-utils@2.0.24

## 1.0.13

### Patch Changes

- @pnpm/cli-utils@2.0.23

## 1.0.12

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/cli-utils@2.0.22
  - @pnpm/fs.find-packages@2.0.7

## 1.0.11

### Patch Changes

- @pnpm/cli-utils@2.0.21

## 1.0.10

### Patch Changes

- @pnpm/cli-utils@2.0.20

## 1.0.9

### Patch Changes

- @pnpm/cli-utils@2.0.19
- @pnpm/fs.find-packages@2.0.6

## 1.0.8

### Patch Changes

- @pnpm/cli-utils@2.0.18

## 1.0.7

### Patch Changes

- @pnpm/cli-utils@2.0.17

## 1.0.6

### Patch Changes

- Updated dependencies [e9aa6f682]
  - @pnpm/fs.find-packages@2.0.5
  - @pnpm/cli-utils@2.0.16

## 1.0.5

### Patch Changes

- 12b0f0976: `pnpm patch` should works correctly when shared-workspace-file is false [#6885](https://github.com/pnpm/pnpm/issues/6885)
  - @pnpm/cli-utils@2.0.15

## 1.0.4

### Patch Changes

- @pnpm/cli-utils@2.0.14

## 1.0.3

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/cli-utils@2.0.13
  - @pnpm/fs.find-packages@2.0.4

## 1.0.2

### Patch Changes

- @pnpm/cli-utils@2.0.12
- @pnpm/fs.find-packages@2.0.3

## 1.0.1

### Patch Changes

- @pnpm/cli-utils@2.0.11

## 1.0.0

### Major Changes

- bd235794d: Package renamed from `@pnpm/find-workspace-packages`.

### Patch Changes

- bd235794d: Output a warning message when "pnpm" or "resolutions" are configured in a non-root workspace project [#6636](https://github.com/pnpm/pnpm/issues/6636)
- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1
  - @pnpm/cli-utils@2.0.10
  - @pnpm/fs.find-packages@2.0.2

## 6.0.9

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/types@9.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/cli-utils@2.0.9
  - @pnpm/fs.find-packages@2.0.1

## 6.0.8

### Patch Changes

- Updated dependencies [ee429b300]
  - @pnpm/cli-utils@2.0.8

## 6.0.7

### Patch Changes

- @pnpm/cli-utils@2.0.7

## 6.0.6

### Patch Changes

- @pnpm/cli-utils@2.0.6

## 6.0.5

### Patch Changes

- @pnpm/cli-utils@2.0.5

## 6.0.4

### Patch Changes

- @pnpm/cli-utils@2.0.4

## 6.0.3

### Patch Changes

- @pnpm/cli-utils@2.0.3

## 6.0.2

### Patch Changes

- @pnpm/cli-utils@2.0.2

## 6.0.1

### Patch Changes

- @pnpm/cli-utils@2.0.1

## 6.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/constants@7.0.0
  - @pnpm/fs.find-packages@2.0.0
  - @pnpm/types@9.0.0
  - @pnpm/cli-utils@2.0.0

## 5.0.42

### Patch Changes

- @pnpm/cli-utils@1.1.7

## 5.0.41

### Patch Changes

- @pnpm/cli-utils@1.1.6

## 5.0.40

### Patch Changes

- @pnpm/cli-utils@1.1.5
- @pnpm/fs.find-packages@1.0.3

## 5.0.39

### Patch Changes

- @pnpm/cli-utils@1.1.4

## 5.0.38

### Patch Changes

- @pnpm/cli-utils@1.1.3

## 5.0.37

### Patch Changes

- Updated dependencies [7d64d757b]
  - @pnpm/cli-utils@1.1.2

## 5.0.36

### Patch Changes

- @pnpm/cli-utils@1.1.1

## 5.0.35

### Patch Changes

- Updated dependencies [0377d9367]
  - @pnpm/cli-utils@1.1.0

## 5.0.34

### Patch Changes

- @pnpm/cli-utils@1.0.34

## 5.0.33

### Patch Changes

- @pnpm/cli-utils@1.0.33

## 5.0.32

### Patch Changes

- @pnpm/cli-utils@1.0.32

## 5.0.31

### Patch Changes

- @pnpm/cli-utils@1.0.31

## 5.0.30

### Patch Changes

- @pnpm/cli-utils@1.0.30

## 5.0.29

### Patch Changes

- @pnpm/cli-utils@1.0.29

## 5.0.28

### Patch Changes

- @pnpm/cli-utils@1.0.28

## 5.0.27

### Patch Changes

- @pnpm/cli-utils@1.0.27

## 5.0.26

### Patch Changes

- @pnpm/cli-utils@1.0.26

## 5.0.25

### Patch Changes

- @pnpm/cli-utils@1.0.25

## 5.0.24

### Patch Changes

- @pnpm/cli-utils@1.0.24

## 5.0.23

### Patch Changes

- @pnpm/cli-utils@1.0.23

## 5.0.22

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/cli-utils@1.0.22
  - @pnpm/fs.find-packages@1.0.2

## 5.0.21

### Patch Changes

- @pnpm/cli-utils@1.0.21

## 5.0.20

### Patch Changes

- @pnpm/cli-utils@1.0.20

## 5.0.19

### Patch Changes

- @pnpm/cli-utils@1.0.19

## 5.0.18

### Patch Changes

- @pnpm/cli-utils@1.0.18

## 5.0.17

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/cli-utils@1.0.17
  - @pnpm/fs.find-packages@1.0.1

## 5.0.16

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/fs.find-packages@1.0.0
  - @pnpm/cli-utils@1.0.16

## 5.0.15

### Patch Changes

- @pnpm/cli-utils@1.0.15

## 5.0.14

### Patch Changes

- @pnpm/cli-utils@1.0.14

## 5.0.13

### Patch Changes

- @pnpm/cli-utils@1.0.13
- find-packages@10.0.4

## 5.0.12

### Patch Changes

- @pnpm/cli-utils@1.0.12

## 5.0.11

### Patch Changes

- @pnpm/cli-utils@1.0.11

## 5.0.10

### Patch Changes

- @pnpm/cli-utils@1.0.10
- find-packages@10.0.3

## 5.0.9

### Patch Changes

- @pnpm/cli-utils@1.0.9

## 5.0.8

### Patch Changes

- @pnpm/cli-utils@1.0.8

## 5.0.7

### Patch Changes

- @pnpm/cli-utils@1.0.7

## 5.0.6

### Patch Changes

- @pnpm/cli-utils@1.0.6

## 5.0.5

### Patch Changes

- 2e9790722: Use deterministic sorting.
- Updated dependencies [2e9790722]
- Updated dependencies [702e847c1]
  - find-packages@10.0.2
  - @pnpm/types@8.9.0
  - @pnpm/cli-utils@1.0.5

## 5.0.4

### Patch Changes

- @pnpm/cli-utils@1.0.4

## 5.0.3

### Patch Changes

- @pnpm/cli-utils@1.0.3

## 5.0.2

### Patch Changes

- @pnpm/cli-utils@1.0.2

## 5.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/cli-utils@1.0.1
  - find-packages@10.0.1

## 5.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - find-packages@10.0.0
  - @pnpm/cli-utils@1.0.0

## 4.0.43

### Patch Changes

- @pnpm/cli-utils@0.7.43
- find-packages@9.0.13

## 4.0.42

### Patch Changes

- @pnpm/cli-utils@0.7.42

## 4.0.41

### Patch Changes

- @pnpm/cli-utils@0.7.41
- find-packages@9.0.12

## 4.0.40

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/cli-utils@0.7.40
  - find-packages@9.0.11

## 4.0.39

### Patch Changes

- @pnpm/cli-utils@0.7.39

## 4.0.38

### Patch Changes

- @pnpm/cli-utils@0.7.38

## 4.0.37

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/cli-utils@0.7.37
  - find-packages@9.0.10

## 4.0.36

### Patch Changes

- @pnpm/cli-utils@0.7.36

## 4.0.35

### Patch Changes

- @pnpm/cli-utils@0.7.35

## 4.0.34

### Patch Changes

- @pnpm/cli-utils@0.7.34

## 4.0.33

### Patch Changes

- @pnpm/cli-utils@0.7.33

## 4.0.32

### Patch Changes

- @pnpm/cli-utils@0.7.32

## 4.0.31

### Patch Changes

- @pnpm/cli-utils@0.7.31

## 4.0.30

### Patch Changes

- @pnpm/cli-utils@0.7.30

## 4.0.29

### Patch Changes

- @pnpm/cli-utils@0.7.29

## 4.0.28

### Patch Changes

- @pnpm/cli-utils@0.7.28

## 4.0.27

### Patch Changes

- @pnpm/cli-utils@0.7.27

## 4.0.26

### Patch Changes

- @pnpm/cli-utils@0.7.26
- find-packages@9.0.9

## 4.0.25

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/cli-utils@0.7.25
  - find-packages@9.0.8

## 4.0.24

### Patch Changes

- @pnpm/cli-utils@0.7.24

## 4.0.23

### Patch Changes

- @pnpm/cli-utils@0.7.23

## 4.0.22

### Patch Changes

- @pnpm/cli-utils@0.7.22
- find-packages@9.0.7

## 4.0.21

### Patch Changes

- @pnpm/cli-utils@0.7.21

## 4.0.20

### Patch Changes

- @pnpm/cli-utils@0.7.20

## 4.0.19

### Patch Changes

- @pnpm/cli-utils@0.7.19

## 4.0.18

### Patch Changes

- @pnpm/cli-utils@0.7.18

## 4.0.17

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/cli-utils@0.7.17

## 4.0.16

### Patch Changes

- @pnpm/cli-utils@0.7.16

## 4.0.15

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/cli-utils@0.7.15
  - find-packages@9.0.6

## 4.0.14

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/cli-utils@0.7.14
  - find-packages@9.0.5

## 4.0.13

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/cli-utils@0.7.13
  - find-packages@9.0.4

## 4.0.12

### Patch Changes

- @pnpm/cli-utils@0.7.12

## 4.0.11

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/cli-utils@0.7.11
  - find-packages@9.0.3

## 4.0.10

### Patch Changes

- @pnpm/cli-utils@0.7.10

## 4.0.9

### Patch Changes

- @pnpm/cli-utils@0.7.9

## 4.0.8

### Patch Changes

- @pnpm/cli-utils@0.7.8

## 4.0.7

### Patch Changes

- @pnpm/cli-utils@0.7.7

## 4.0.6

### Patch Changes

- @pnpm/cli-utils@0.7.6

## 4.0.5

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/cli-utils@0.7.5

## 4.0.4

### Patch Changes

- @pnpm/cli-utils@0.7.4

## 4.0.3

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/cli-utils@0.7.3
  - find-packages@9.0.2

## 4.0.2

### Patch Changes

- @pnpm/cli-utils@0.7.2

## 4.0.1

### Patch Changes

- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0
  - @pnpm/cli-utils@0.7.1
  - find-packages@9.0.1

## 4.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/constants@6.0.0
  - find-packages@9.0.0
  - @pnpm/cli-utils@0.7.0

## 3.1.42

### Patch Changes

- @pnpm/cli-utils@0.6.50
- find-packages@8.0.13

## 3.1.41

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/cli-utils@0.6.49
  - find-packages@8.0.12

## 3.1.40

### Patch Changes

- @pnpm/cli-utils@0.6.48

## 3.1.39

### Patch Changes

- @pnpm/cli-utils@0.6.47

## 3.1.38

### Patch Changes

- @pnpm/cli-utils@0.6.46

## 3.1.37

### Patch Changes

- @pnpm/cli-utils@0.6.45

## 3.1.36

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/cli-utils@0.6.44
  - find-packages@8.0.11

## 3.1.35

### Patch Changes

- @pnpm/cli-utils@0.6.43

## 3.1.34

### Patch Changes

- @pnpm/cli-utils@0.6.42

## 3.1.33

### Patch Changes

- @pnpm/cli-utils@0.6.41

## 3.1.32

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/cli-utils@0.6.40
  - find-packages@8.0.10

## 3.1.31

### Patch Changes

- @pnpm/cli-utils@0.6.39

## 3.1.30

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/cli-utils@0.6.38
  - find-packages@8.0.9

## 3.1.29

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/cli-utils@0.6.37
  - find-packages@8.0.8

## 3.1.28

### Patch Changes

- @pnpm/cli-utils@0.6.36

## 3.1.27

### Patch Changes

- @pnpm/cli-utils@0.6.35

## 3.1.26

### Patch Changes

- @pnpm/cli-utils@0.6.34

## 3.1.25

### Patch Changes

- @pnpm/cli-utils@0.6.33

## 3.1.24

### Patch Changes

- @pnpm/cli-utils@0.6.32

## 3.1.23

### Patch Changes

- @pnpm/cli-utils@0.6.31

## 3.1.22

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/cli-utils@0.6.30
  - find-packages@8.0.7

## 3.1.21

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/cli-utils@0.6.29
  - find-packages@8.0.6

## 3.1.20

### Patch Changes

- @pnpm/cli-utils@0.6.28

## 3.1.19

### Patch Changes

- @pnpm/cli-utils@0.6.27

## 3.1.18

### Patch Changes

- @pnpm/cli-utils@0.6.26

## 3.1.17

### Patch Changes

- @pnpm/cli-utils@0.6.25

## 3.1.16

### Patch Changes

- @pnpm/cli-utils@0.6.24

## 3.1.15

### Patch Changes

- @pnpm/cli-utils@0.6.23

## 3.1.14

### Patch Changes

- @pnpm/cli-utils@0.6.22

## 3.1.13

### Patch Changes

- @pnpm/cli-utils@0.6.21

## 3.1.12

### Patch Changes

- @pnpm/cli-utils@0.6.20

## 3.1.11

### Patch Changes

- @pnpm/cli-utils@0.6.19

## 3.1.10

### Patch Changes

- @pnpm/cli-utils@0.6.18

## 3.1.9

### Patch Changes

- @pnpm/cli-utils@0.6.17

## 3.1.8

### Patch Changes

- @pnpm/cli-utils@0.6.16

## 3.1.7

### Patch Changes

- @pnpm/cli-utils@0.6.15

## 3.1.6

### Patch Changes

- @pnpm/cli-utils@0.6.14

## 3.1.5

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/cli-utils@0.6.13
  - find-packages@8.0.5

## 3.1.4

### Patch Changes

- @pnpm/cli-utils@0.6.12

## 3.1.3

### Patch Changes

- @pnpm/cli-utils@0.6.11

## 3.1.2

### Patch Changes

- @pnpm/cli-utils@0.6.10

## 3.1.1

### Patch Changes

- @pnpm/cli-utils@0.6.9

## 3.1.0

### Minor Changes

- a5bde0aa2: Export findWorkspacePackagesNoCheck() for finding packages and skipping engine checks.

## 3.0.8

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/cli-utils@0.6.8
  - find-packages@8.0.4

## 3.0.7

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/cli-utils@0.6.7
  - find-packages@8.0.3

## 3.0.6

### Patch Changes

- @pnpm/cli-utils@0.6.6

## 3.0.5

### Patch Changes

- @pnpm/cli-utils@0.6.5

## 3.0.4

### Patch Changes

- @pnpm/cli-utils@0.6.4

## 3.0.3

### Patch Changes

- @pnpm/cli-utils@0.6.3
- find-packages@8.0.2

## 3.0.2

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/cli-utils@0.6.2
  - find-packages@8.0.1

## 3.0.1

### Patch Changes

- @pnpm/cli-utils@0.6.1

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - @pnpm/cli-utils@0.6.0
  - find-packages@8.0.0
  - @pnpm/types@7.0.0

## 2.3.42

### Patch Changes

- @pnpm/cli-utils@0.5.4

## 2.3.41

### Patch Changes

- @pnpm/cli-utils@0.5.3

## 2.3.40

### Patch Changes

- @pnpm/cli-utils@0.5.2

## 2.3.39

### Patch Changes

- Updated dependencies [3be2b1773]
  - @pnpm/cli-utils@0.5.1

## 2.3.38

### Patch Changes

- Updated dependencies [cb040ae18]
  - @pnpm/cli-utils@0.5.0

## 2.3.37

### Patch Changes

- @pnpm/cli-utils@0.4.51
- find-packages@7.0.24

## 2.3.36

### Patch Changes

- @pnpm/cli-utils@0.4.50

## 2.3.35

### Patch Changes

- @pnpm/cli-utils@0.4.49

## 2.3.34

### Patch Changes

- @pnpm/cli-utils@0.4.48

## 2.3.33

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
  - @pnpm/types@6.4.0
  - @pnpm/cli-utils@0.4.47
  - find-packages@7.0.23

## 2.3.32

### Patch Changes

- @pnpm/cli-utils@0.4.46

## 2.3.31

### Patch Changes

- @pnpm/cli-utils@0.4.45

## 2.3.30

### Patch Changes

- @pnpm/cli-utils@0.4.44

## 2.3.29

### Patch Changes

- @pnpm/cli-utils@0.4.43

## 2.3.28

### Patch Changes

- @pnpm/cli-utils@0.4.42

## 2.3.27

### Patch Changes

- @pnpm/cli-utils@0.4.41

## 2.3.26

### Patch Changes

- @pnpm/cli-utils@0.4.40

## 2.3.25

### Patch Changes

- @pnpm/cli-utils@0.4.39

## 2.3.24

### Patch Changes

- @pnpm/cli-utils@0.4.38
- find-packages@7.0.22

## 2.3.23

### Patch Changes

- @pnpm/cli-utils@0.4.37
- find-packages@7.0.21

## 2.3.22

### Patch Changes

- @pnpm/cli-utils@0.4.36
- find-packages@7.0.20

## 2.3.21

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/cli-utils@0.4.35
  - find-packages@7.0.19

## 2.3.20

### Patch Changes

- @pnpm/cli-utils@0.4.34

## 2.3.19

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/cli-utils@0.4.33
  - find-packages@7.0.18

## 2.3.18

### Patch Changes

- @pnpm/cli-utils@0.4.32

## 2.3.17

### Patch Changes

- @pnpm/cli-utils@0.4.31
- find-packages@7.0.17

## 2.3.16

### Patch Changes

- @pnpm/cli-utils@0.4.30

## 2.3.15

### Patch Changes

- @pnpm/cli-utils@0.4.29

## 2.3.14

### Patch Changes

- @pnpm/cli-utils@0.4.28

## 2.3.13

### Patch Changes

- @pnpm/cli-utils@0.4.27

## 2.3.12

### Patch Changes

- @pnpm/cli-utils@0.4.26

## 2.3.11

### Patch Changes

- @pnpm/cli-utils@0.4.25
- find-packages@7.0.16

## 2.3.10

### Patch Changes

- @pnpm/cli-utils@0.4.24

## 2.3.9

### Patch Changes

- @pnpm/cli-utils@0.4.23

## 2.3.8

### Patch Changes

- @pnpm/cli-utils@0.4.22
- find-packages@7.0.15

## 2.3.7

### Patch Changes

- @pnpm/cli-utils@0.4.21

## 2.3.6

### Patch Changes

- @pnpm/cli-utils@0.4.20

## 2.3.5

### Patch Changes

- @pnpm/cli-utils@0.4.19
- find-packages@7.0.14

## 2.3.4

### Patch Changes

- @pnpm/cli-utils@0.4.18
- find-packages@7.0.13

## 2.3.3

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
  - @pnpm/cli-utils@0.4.17

## 2.3.2

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/cli-utils@0.4.16

## 2.3.1

### Patch Changes

- @pnpm/cli-utils@0.4.15

## 2.3.0

### Minor Changes

- faae9a93c: A project with no version field is treated as if it had version 0.0.0.

### Patch Changes

- @pnpm/cli-utils@0.4.14

## 2.2.11

### Patch Changes

- @pnpm/cli-utils@0.4.13

## 2.2.10

### Patch Changes

- @pnpm/cli-utils@0.4.12

## 2.2.9

### Patch Changes

- @pnpm/cli-utils@0.4.11

## 2.2.8

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/cli-utils@0.4.10
  - find-packages@7.0.12

## 2.2.7

### Patch Changes

- Updated dependencies [1520e3d6f]
  - find-packages@7.0.11

## 2.2.6

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/config@9.2.0
  - @pnpm/cli-utils@0.4.9
  - find-packages@7.0.10

## 2.2.5

### Patch Changes

- Updated dependencies [e934b1a48]
  - @pnpm/cli-utils@0.4.8
  - find-packages@7.0.9

## 2.2.4

### Patch Changes

- @pnpm/cli-utils@0.4.7

## 2.2.3

### Patch Changes

- Updated dependencies [ffddf34a8]
  - @pnpm/config@9.1.0
  - @pnpm/cli-utils@0.4.6

## 2.2.2

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [da091c711]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/config@9.0.0
  - @pnpm/types@6.0.0
  - @pnpm/cli-utils@0.4.5
  - find-packages@7.0.8

## 2.2.2-alpha.2

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/cli-utils@0.4.5-alpha.2

## 2.2.2-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - find-packages@7.0.8-alpha.0

## 2.2.2-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/cli-utils@0.4.5-alpha.0

## 2.2.1

### Patch Changes

- @pnpm/cli-utils@0.4.4
- find-packages@7.0.7
