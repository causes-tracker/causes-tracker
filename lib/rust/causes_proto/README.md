# causes\_proto

Generated Rust bindings for the Causes gRPC protocol buffers.
Source `.proto` files live in `//proto`; this crate contains only the generated output.

The generated code is checked in rather than produced at build time so that IDEs can resolve types without running Bazel.
Regenerate after changing `.proto` files:

```sh
bazel run //tools:proto_gen
```
