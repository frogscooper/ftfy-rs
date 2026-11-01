# Releasing

Python wheels go to PyPI through GitHub Actions ([`release.yml`](.github/workflows/release.yml)). The Rust crate goes to crates.io by hand.

## One-time setup

1. **PyPI trusted publisher.** Log in to [pypi.org](https://pypi.org), go to *Your account → Publishing*, and add a pending publisher:

   | field             | value         |
   |-------------------|---------------|
   | PyPI project name | `ftfy-rs`     |
   | Owner             | `frogscooper` |
   | Repository name   | `ftfy-rs`     |
   | Workflow name     | `release.yml` |
   | Environment name  | `pypi`        |

   No API token is needed. The first publish creates the project.

2. **GitHub environment.** In the repository, go to *Settings → Environments → New environment* and create one named `pypi`. Adding yourself as a required reviewer is optional, but it means every release waits for your click before uploading.

3. **crates.io.** Log in at [crates.io](https://crates.io) with GitHub, create an API token, and run `cargo login`.

## Every release

1. Bump the version in [`Cargo.toml`](Cargo.toml) (`[workspace.package] version`) and in [`python/ftfy_rs/__init__.py`](python/ftfy_rs/__init__.py) (`__version__`). A test checks that the two match.
2. Move the changes in [`CHANGELOG.md`](CHANGELOG.md) under the new version and date.
3. **Dry run:** push, then run the *Release* workflow by hand from the Actions tab. It builds and tests every wheel without publishing anything.
4. Tag and push:

   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

   The workflow builds wheels for Linux (glibc and musl, x86_64 and aarch64), macOS (Intel and Apple silicon) and Windows (x64 and Arm), for CPython 3.9–3.14, tests them, and publishes them with the sdist.

5. Publish the Rust crate:

   ```bash
   cargo publish -p ftfy-rs
   ```

A release can't be replaced once it's on PyPI or crates.io (only yanked), so do the dry run first.
