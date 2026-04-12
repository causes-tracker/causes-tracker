# docs

Documentation site built with [Zensical](https://github.com/zensical/zensical).
Content lives in `designdocs/` (hand-written) and is supplemented by auto-generated proto reference docs from `//proto`.

The site is built entirely inside Bazel's sandbox.
`mkdocs.yml` paths are rewritten at build time because Zensical silently finds zero pages when following double-symlink chains (sandbox -> execroot -> source).
Theme assets are copied manually from runfiles for the same reason.

## Local preview

```sh
bazel run //docs:serve_docs
```
