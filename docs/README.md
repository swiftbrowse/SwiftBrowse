# Documentation site

Markdown pages in `pages/`, built into a static site in `dist/` by `build.mjs`. Two pages are generated from the binary itself: the operations reference (`swiftbrowse sdk --schema`) and the command-line reference (`--help`), so build with the binary of the version you are documenting.

```sh
cargo build --release
cd docs && npm install && node build.mjs     # or: node build.mjs path/to/swiftbrowse
```

`{{version}}` in a page is replaced with the binary's version. Links between pages use the `.md` file names (`[Assertions](assertions.md)`); the build rewrites them. The sidebar order is `NAV` in `build.mjs`.

The site is hosted on here.now at https://steady-ember-82q7.here.now/. To update it after a build, publish `dist/` to the same slug:

```sh
~/.agents/skills/here-now/scripts/publish.sh dist --slug steady-ember-82q7 --client claude-code
```
