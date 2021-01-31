Upstream packages are identified by a tuple:

 * family ("python", "rust", "perl")
 * identifier ("dulwich")

Dependencies are a three-tuple:

 * kind ("runtime", "build", "test")
 * package ("python", "dulwich")
 * version constraints

Note that version comparisons are specific to the package family.
