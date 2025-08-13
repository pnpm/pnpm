# @pnpm/assert-project

## 4.0.16

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [f91922c]
  - @pnpm/constants@1001.3.0
  - @pnpm/lockfile.types@1002.0.0
  - @pnpm/assert-store@2.0.16

## 4.0.15

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/lockfile.types@1001.1.0
  - @pnpm/constants@1001.2.0
  - @pnpm/modules-yaml@1000.3.4
  - @pnpm/assert-store@2.0.15

## 4.0.14

### Patch Changes

- @pnpm/assert-store@2.0.14

## 4.0.13

### Patch Changes

- @pnpm/assert-store@2.0.13

## 4.0.12

### Patch Changes

- Updated dependencies [5ec7255]
  - @pnpm/types@1000.6.0
  - @pnpm/lockfile.types@1001.0.8
  - @pnpm/modules-yaml@1000.3.3
  - @pnpm/assert-store@2.0.12

## 4.0.11

### Patch Changes

- Updated dependencies [5b73df1]
  - @pnpm/types@1000.5.0
  - @pnpm/lockfile.types@1001.0.7
  - @pnpm/modules-yaml@1000.3.2
  - @pnpm/assert-store@2.0.11

## 4.0.10

### Patch Changes

- @pnpm/assert-store@2.0.10

## 4.0.9

### Patch Changes

- Updated dependencies [750ae7d]
  - @pnpm/types@1000.4.0
  - @pnpm/lockfile.types@1001.0.6
  - @pnpm/modules-yaml@1000.3.1
  - @pnpm/assert-store@2.0.9

## 4.0.8

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [64f6b4f]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0
  - @pnpm/modules-yaml@1000.3.0
  - @pnpm/lockfile.types@1001.0.5
  - @pnpm/assert-store@2.0.8

## 4.0.7

### Patch Changes

- Updated dependencies [d612dcf]
- Updated dependencies [d612dcf]
  - @pnpm/modules-yaml@1000.2.0
  - @pnpm/assert-store@2.0.7

## 4.0.6

### Patch Changes

- Updated dependencies [a5e4965]
  - @pnpm/types@1000.2.1
  - @pnpm/lockfile.types@1001.0.4
  - @pnpm/modules-yaml@1000.1.4
  - @pnpm/assert-store@2.0.6

## 4.0.5

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0
  - @pnpm/lockfile.types@1001.0.3
  - @pnpm/modules-yaml@1000.1.3
  - @pnpm/assert-store@2.0.5

## 4.0.4

### Patch Changes

- Updated dependencies [9a44e6c]
- Updated dependencies [b562deb]
  - @pnpm/constants@1001.1.0
  - @pnpm/types@1000.1.1
  - @pnpm/assert-store@2.0.4
  - @pnpm/lockfile.types@1001.0.2
  - @pnpm/modules-yaml@1000.1.2

## 4.0.3

### Patch Changes

- @pnpm/assert-store@2.0.3

## 4.0.2

### Patch Changes

- Updated dependencies [9591a18]
  - @pnpm/types@1000.1.0
  - @pnpm/lockfile.types@1001.0.1
  - @pnpm/modules-yaml@1000.1.1
  - @pnpm/assert-store@2.0.2

## 4.0.1

### Patch Changes

- Updated dependencies [4771813]
  - @pnpm/modules-yaml@1000.1.0

## 4.0.0

### Major Changes

- a76da0c: Removed lockfile conversion from v6 to v9. If you need to convert lockfile v6 to v9, use pnpm CLI v9.

### Patch Changes

- Updated dependencies [d2e83b0]
- Updated dependencies [6483b64]
- Updated dependencies [a76da0c]
  - @pnpm/constants@1001.0.0
  - @pnpm/lockfile.types@1001.0.0
  - @pnpm/assert-store@2.0.1

## 3.0.0

### Major Changes

- d433cb9: Some registries allow identical content to be published under different package names or versions. To accommodate this, index files in the store are now stored using both the content hash and package identifier.

  This approach ensures that we can:

  1. Validate that the integrity in the lockfile corresponds to the correct package,
     which might not be the case after a poorly resolved Git conflict.
  2. Allow the same content to be referenced by different packages or different versions of the same package.

  Related PR: [#8510](https://github.com/pnpm/pnpm/pull/8510)
  Related issue: [#8204](https://github.com/pnpm/pnpm/issues/8204)

### Patch Changes

- Updated dependencies [19d5b51]
- Updated dependencies [8108680]
- Updated dependencies [d433cb9]
- Updated dependencies [c4f5231]
  - @pnpm/constants@10.0.0
  - @pnpm/assert-store@2.0.0

## 2.3.62

### Patch Changes

- @pnpm/assert-store@1.0.92

## 2.3.61

### Patch Changes

- @pnpm/assert-store@1.0.91

## 2.3.60

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/constants@9.0.0

## 2.3.59

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0
  - @pnpm/lockfile.types@1.0.3
  - @pnpm/modules-yaml@13.1.7
  - @pnpm/assert-store@1.0.90

## 2.3.58

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/lockfile.types@1.0.2
  - @pnpm/modules-yaml@13.1.6
  - @pnpm/assert-store@1.0.89

## 2.3.57

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/lockfile.types@1.0.1
  - @pnpm/types@12.0.0
  - @pnpm/modules-yaml@13.1.5
  - @pnpm/assert-store@1.0.88

## 2.3.56

### Patch Changes

- Updated dependencies [797ef0f]
  - @pnpm/lockfile.types@1.0.0

## 2.3.55

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0
  - @pnpm/lockfile-types@7.1.3
  - @pnpm/modules-yaml@13.1.4
  - @pnpm/assert-store@1.0.87

## 2.3.54

### Patch Changes

- @pnpm/assert-store@1.0.86

## 2.3.53

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/modules-yaml@13.1.3
  - @pnpm/assert-store@1.0.85

## 2.3.52

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/modules-yaml@13.1.2
  - @pnpm/assert-store@1.0.84

## 2.3.51

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/lockfile-types@7.1.0

## 2.3.50

### Patch Changes

- @pnpm/assert-store@1.0.83

## 2.3.49

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/modules-yaml@13.1.1
  - @pnpm/assert-store@1.0.82

## 2.3.48

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/modules-yaml@13.1.0

## 2.3.47

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [c692f80]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/modules-yaml@13.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/assert-store@1.0.81

## 2.3.46

### Patch Changes

- @pnpm/assert-store@1.0.80

## 2.3.45

### Patch Changes

- Updated dependencies [d349bc3a2]
  - @pnpm/modules-yaml@12.1.7

## 2.3.44

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/assert-store@1.0.79
  - @pnpm/modules-yaml@12.1.6

## 2.3.43

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/modules-yaml@12.1.5
  - @pnpm/assert-store@1.0.78

## 2.3.42

### Patch Changes

- @pnpm/assert-store@1.0.77

## 2.3.41

### Patch Changes

- @pnpm/assert-store@1.0.76

## 2.3.40

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/modules-yaml@12.1.4
  - @pnpm/assert-store@1.0.75

## 2.3.39

### Patch Changes

- @pnpm/assert-store@1.0.74

## 2.3.38

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/modules-yaml@12.1.3
  - @pnpm/assert-store@1.0.73

## 2.3.37

### Patch Changes

- @pnpm/assert-store@1.0.72

## 2.3.36

### Patch Changes

- @pnpm/assert-store@1.0.71

## 2.3.35

### Patch Changes

- @pnpm/assert-store@1.0.70

## 2.3.34

### Patch Changes

- @pnpm/assert-store@1.0.69

## 2.3.33

### Patch Changes

- @pnpm/assert-store@1.0.68

## 2.3.32

### Patch Changes

- @pnpm/assert-store@1.0.67

## 2.3.31

### Patch Changes

- @pnpm/assert-store@1.0.66

## 2.3.30

### Patch Changes

- @pnpm/assert-store@1.0.65

## 2.3.29

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/modules-yaml@12.1.2
  - @pnpm/assert-store@1.0.64

## 2.3.28

### Patch Changes

- @pnpm/assert-store@1.0.63

## 2.3.27

### Patch Changes

- @pnpm/assert-store@1.0.62

## 2.3.26

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1

## 2.3.25

### Patch Changes

- @pnpm/assert-store@1.0.61

## 2.3.24

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/modules-yaml@12.1.1
  - @pnpm/assert-store@1.0.60

## 2.3.23

### Patch Changes

- Updated dependencies [e6b83c84e]
  - @pnpm/modules-yaml@12.1.0

## 2.3.22

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [eceaa8b8b]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/modules-yaml@12.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/types@9.0.0
  - @pnpm/assert-store@1.0.59

## 2.3.21

### Patch Changes

- @pnpm/assert-store@1.0.58

## 2.3.20

### Patch Changes

- @pnpm/assert-store@1.0.57

## 2.3.19

### Patch Changes

- @pnpm/assert-store@1.0.56

## 2.3.18

### Patch Changes

- @pnpm/assert-store@1.0.55

## 2.3.17

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/assert-store@1.0.54

## 2.3.16

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/modules-yaml@11.1.0
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/assert-store@1.0.53

## 2.3.15

### Patch Changes

- @pnpm/assert-store@1.0.52

## 2.3.14

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/lockfile-types@4.3.5
  - @pnpm/modules-yaml@11.0.2
  - @pnpm/assert-store@1.0.51

## 2.3.13

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/lockfile-types@4.3.4
  - @pnpm/modules-yaml@11.0.1
  - @pnpm/assert-store@1.0.50

## 2.3.12

### Patch Changes

- Updated dependencies [72f7d6b3b]
  - @pnpm/modules-yaml@11.0.0
  - @pnpm/assert-store@1.0.49

## 2.3.11

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/lockfile-types@4.3.3
  - @pnpm/modules-yaml@10.0.8
  - @pnpm/assert-store@1.0.48

## 2.3.10

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/lockfile-types@4.3.2
  - @pnpm/modules-yaml@10.0.7
  - @pnpm/assert-store@1.0.47

## 2.3.9

### Patch Changes

- @pnpm/assert-store@1.0.46

## 2.3.8

### Patch Changes

- @pnpm/assert-store@1.0.45

## 2.3.7

### Patch Changes

- @pnpm/assert-store@1.0.44

## 2.3.6

### Patch Changes

- @pnpm/assert-store@1.0.43

## 2.3.5

### Patch Changes

- @pnpm/assert-store@1.0.42

## 2.3.4

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/lockfile-types@4.3.1
  - @pnpm/modules-yaml@10.0.6
  - @pnpm/assert-store@1.0.41

## 2.3.3

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0

## 2.3.2

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - @pnpm/modules-yaml@10.0.5
  - @pnpm/assert-store@1.0.40

## 2.3.1

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - @pnpm/modules-yaml@10.0.4
  - @pnpm/assert-store@1.0.39

## 2.3.0

### Minor Changes

- 56cf04cb3: New settings added: use-git-branch-lockfile, merge-git-branch-lockfiles, merge-git-branch-lockfiles-branch-pattern.

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/lockfile-types@4.0.3
  - @pnpm/modules-yaml@10.0.3
  - @pnpm/assert-store@1.0.38

## 2.2.23

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/lockfile-types@4.0.2
  - @pnpm/modules-yaml@10.0.2
  - @pnpm/assert-store@1.0.37

## 2.2.22

### Patch Changes

- @pnpm/assert-store@1.0.36

## 2.2.21

### Patch Changes

- @pnpm/assert-store@1.0.35

## 2.2.20

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/lockfile-types@4.0.1
  - @pnpm/modules-yaml@10.0.1
  - @pnpm/assert-store@1.0.34

## 2.2.19

### Patch Changes

- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0

## 2.2.18

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/lockfile-types@4.0.0
  - @pnpm/modules-yaml@10.0.0
  - @pnpm/assert-store@1.0.33

## 2.2.17

### Patch Changes

- @pnpm/assert-store@1.0.32

## 2.2.16

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - @pnpm/modules-yaml@9.1.1
  - @pnpm/assert-store@1.0.31

## 2.2.15

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/modules-yaml@9.1.0

## 2.2.14

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/lockfile-types@3.1.5
  - @pnpm/modules-yaml@9.0.11
  - @pnpm/assert-store@1.0.30

## 2.2.13

### Patch Changes

- Updated dependencies [7375396db]
  - @pnpm/modules-yaml@9.0.10

## 2.2.12

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/lockfile-types@3.1.4
  - @pnpm/modules-yaml@9.0.9
  - @pnpm/assert-store@1.0.29

## 2.2.11

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/lockfile-types@3.1.3
  - @pnpm/modules-yaml@9.0.8
  - @pnpm/assert-store@1.0.28

## 2.2.10

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/lockfile-types@3.1.2
  - @pnpm/modules-yaml@9.0.7
  - @pnpm/assert-store@1.0.27

## 2.2.9

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/lockfile-types@3.1.1
  - @pnpm/modules-yaml@9.0.6
  - @pnpm/assert-store@1.0.26

## 2.2.8

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-types@3.1.0
  - @pnpm/modules-yaml@9.0.5
  - @pnpm/assert-store@1.0.25

## 2.2.7

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/modules-yaml@9.0.4
  - @pnpm/assert-store@1.0.24

## 2.2.6

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/modules-yaml@9.0.3
  - @pnpm/assert-store@1.0.23

## 2.2.5

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/modules-yaml@9.0.2
  - @pnpm/assert-store@1.0.22

## 2.2.4

### Patch Changes

- @pnpm/assert-store@1.0.21

## 2.2.3

### Patch Changes

- @pnpm/assert-store@1.0.20

## 2.2.2

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/modules-yaml@9.0.1
  - @pnpm/assert-store@1.0.19

## 2.2.1

### Patch Changes

- @pnpm/assert-store@1.0.18

## 2.2.0

### Minor Changes

- 735d2ac79: expose dir of project

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [78470a32d]
  - @pnpm/constants@5.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/modules-yaml@9.0.0
  - @pnpm/types@7.0.0
  - @pnpm/assert-store@1.0.17

## 2.1.16

### Patch Changes

- @pnpm/assert-store@1.0.16

## 2.1.15

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - @pnpm/modules-yaml@8.0.6
  - @pnpm/assert-store@1.0.15

## 2.1.14

### Patch Changes

- @pnpm/assert-store@1.0.14

## 2.1.13

### Patch Changes

- Updated dependencies [09492b7b4]
  - @pnpm/modules-yaml@8.0.5

## 2.1.12

### Patch Changes

- @pnpm/assert-store@1.0.13

## 2.1.11

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - @pnpm/modules-yaml@8.0.4
  - @pnpm/assert-store@1.0.12

## 2.1.10

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/modules-yaml@8.0.3
  - @pnpm/assert-store@1.0.11

## 2.1.9

### Patch Changes

- @pnpm/assert-store@1.0.10

## 2.1.8

### Patch Changes

- @pnpm/assert-store@1.0.9

## 2.1.7

### Patch Changes

- @pnpm/assert-store@1.0.8

## 2.1.6

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/modules-yaml@8.0.2
  - @pnpm/assert-store@1.0.7

## 2.1.5

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/assert-store@1.0.6

## 2.1.4

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/assert-store@1.0.5

## 2.1.3

### Patch Changes

- @pnpm/assert-store@1.0.4

## 2.1.2

### Patch Changes

- @pnpm/assert-store@1.0.3

## 2.1.1

### Patch Changes

- @pnpm/assert-store@1.0.2

## 2.1.0

### Minor Changes

- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [da091c711]
- Updated dependencies [802d145fc]
- Updated dependencies [6a8a97eee]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - @pnpm/assert-store@1.0.1

## 2.1.0-alpha.2

### Patch Changes

- Updated dependencies [ca9f50844]
- Updated dependencies [6a8a97eee]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/assert-store@1.0.1-alpha.2

## 2.1.0-alpha.1

### Minor Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/assert-store@1.0.1-alpha.1

## 2.0.2-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/assert-store@1.0.1-alpha.0

## 2.0.1

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/modules-yaml@6.0.2
