# Local Slang patches

Empty on purpose: upstream Slang is used unmodified by the optional compiler
oracles in `tools/oracles/`. Put a patch here only as a last resort, named
`NNNN-<upstream issue>-<short-name>.patch`, and follow
[../README.md](../README.md#adding-a-patch). `*.patch` files here are applied
in name order after CMake fetches the pinned commit.
