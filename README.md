# patch-crate

Fork of [mokeyish/cargo-patch-crate](https://github.com/mokeyish/cargo-patch-crate).

Differences from upstream:

- Uses direct `Cargo.toml` / `Cargo.lock` parsing instead of `cargo_metadata`. Smaller binary, no runtime cargo-resolver dep.
- Tolerates `[patch.crates-io]` entries that point to a missing `target/patch/` path on first run.
- `cargo patch-crate` is an idempotent bootstrap: one invocation, any starting state → `target/patch/` populated and patched.

## Index

- [Using patch-crate](#using-patch-crate)
- [Command reference](#command-reference)
- [Dev](#dev)
- [Credits](#credits)
- [License](#license)

## Using patch-crate

`patch-crate` lets you keep local fixes to a dependency's source, shared via a committed `.patch` file.

### Install

```sh
cargo install --git https://github.com/jspaezp/cargo-patch-crate
```

### Worked example: patching the `home` crate

1. **Declare the patch in `Cargo.toml`.**

   ```toml
   [dependencies]
   home = "0.5"

   [package.metadata.patch]
   crates = ["home"]

   [patch.crates-io]
   home = { path = "./target/patch/home-0.5.12" }
   ```

2. **Bootstrap.** On a fresh clone, run:

   ```sh
   cargo patch-crate
   ```

   This populates `target/patch/home-0.5.12/` with the crate source and applies any patches under `patches/`. Safe to re-run: it's an idempotent reconcile.

3. **Edit the patched source.**

   Open `target/patch/home-0.5.12/src/lib.rs` and add:

   ```rust
   pub fn i_was_patched_correctly() -> &'static str {
       "i was patched correctly"
   }
   ```

4. **Generate the patch file.**

   ```sh
   cargo patch-crate home
   ```

   This writes `patches/home+0.5.12.patch`.

5. **Commit the patch.**

   ```sh
   git add patches/home+0.5.12.patch
   git commit -m "patch home: add i_was_patched_correctly"
   ```

6. **Use the patched function in your code.**

   ```rust
   fn main() {
       println!("{}", home::i_was_patched_correctly());
   }
   ```

### Keeping `target/patch/` fresh after edits

You can have `build.rs` call `patch_crate::run()` so that edits to patch files re-apply automatically:

```rust
// build.rs
fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    patch_crate::run().expect("Failed while patching");
}
```

```toml
# Cargo.toml
[build-dependencies]
patch-crate = { git = "https://github.com/jspaezp/cargo-patch-crate" }
```

**Caveat — this does not replace the bootstrap step.** Cargo validates `[patch.crates-io]` paths during manifest load, before any `build.rs` is compiled or run. On a fresh clone `cargo build` will fail until you run `cargo patch-crate` once. The `build.rs` hook only helps after that.

## Command reference

- `cargo patch-crate`
  Idempotent reconcile. Applies every patch under `patches/` to `target/patch/<crate>-<version>/`. Populates missing source trees from the local registry cache or by HTTP fetch from crates.io.

- `cargo patch-crate --force`
  Wipes `target/patch/` first, then reconciles from scratch.

- `cargo patch-crate <name>[@<version>] [...]`
  Generates a new `patches/<name>+<version>.patch` from the current state of `target/patch/<name>-<version>/`.

## Dev

Run the test suite:

```sh
cargo test
```

Run the slower end-to-end check (nested `cargo run`, needs network if the registry cache is cold):

```sh
cargo test -- --ignored
```

## Credits

- [itmettkeDE/cargo-patch](https://github.com/itmettkeDE/cargo-patch)
- [ds300/patch-package](https://github.com/ds300/patch-package)
- Upstream: [mokeyish/cargo-patch-crate](https://github.com/mokeyish/cargo-patch-crate)

## License

Dual licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.
