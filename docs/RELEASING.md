# Releasing Obscura Server

The `main` ruleset requires **Test & Check** before a version change can land.
Prepare the version bump on a branch and merge it through a pull request. Do not
push a version commit directly to `main`.

## Prepare the version pull request

1. Start from the latest `main` and create a release branch, for example
   `release/v0.9.6`.
2. Bump the crate version with `cargo set-version --bump patch` (or `minor` /
   `major`). This command is provided by `cargo-edit`; install it with
   `cargo install cargo-edit --locked` if it is not already available.
3. Run `cargo check` to update `Cargo.lock`, then `cargo check --locked` to
   verify the committed lockfile is usable. Commit **both** `Cargo.toml` and
   `Cargo.lock`, push the branch, and open a pull request against `main`.
4. Wait for **Test & Check** to pass, review the version change, and merge the
   pull request. Confirm the new version is present on `main`.

## Tag and publish

Create an annotated `v<version>` tag on the **merged version-bump commit**, not
on the release branch. For example, after merging the `0.9.6` pull request:

```bash
git fetch origin main --tags
git switch main
git merge --ff-only origin/main
git tag -a v0.9.6 -m "Release v0.9.6"
git push origin refs/tags/v0.9.6
```

Before tagging, check that `HEAD` is the merged release commit and that
`Cargo.toml` contains `version = "0.9.6"`. If `main` has advanced beyond that
commit, tag the pull request's merge commit explicitly instead. Push the tag
with your Git credentials: pushes made using `GITHUB_TOKEN` do not trigger
downstream workflows.

The tag starts **Publish Release**, whose independent jobs publish the crate
to crates.io, the image to `ghcr.io/obscura-messaging/obscura-server` (version,
minor, and `latest` tags), and a GitHub release. Verify all three jobs. If a
job fails, diagnose it before rerunning the failed job; do not bump or retag
the version. If the tag push did not start the workflow, dispatch **Publish
Release** manually with the tag as its ref.

The first image published under the organization creates a new GHCR package.
New packages default to **private**, even for public repositories. An
organization owner must set the package visibility to **Public** in GitHub's
package settings. If GitHub says public visibility is disabled by organization
administrators, the owner must first enable **Public** under organization
**Settings > Packages > Package creation**. That policy permits members to
create other public packages too. Verify the versioned and `latest` images are
anonymously pullable before updating unauthenticated consumers such as the Helm
chart and native integration CI. The old personal-account image does not move
or redirect.
