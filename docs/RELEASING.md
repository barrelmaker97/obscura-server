# Releasing Obscura Server

Releases are managed via GitHub Actions to ensure consistency and security.

## Workflow

1. Go to the **Actions** tab in the GitHub repository.
2. Select the **Bump Version & Tag** workflow on the left sidebar.
3. Click **Run workflow**.
4. Select the **Bump Type** from the dropdown (`patch`, `minor`, or `major`).
5. Click **Run workflow**.

## Automated Actions

The system will automatically perform the following steps:

1. **Bump Version**: Update the version in `Cargo.toml` based on the selected bump type.
2. **Check, Commit & Tag**: Commit the version change to a temporary release-candidate branch, run the required CI check there, then push that checked commit and its git tag together to `main`. A failed check leaves `main` and the release tag unchanged.
3. **Publish**: Dispatch the **Publish Release** workflow for the tag to build and publish artifacts:
   - **Crates.io**: The updated crate is published.
   - **GHCR (GitHub Container Registry)**: A new Docker image is built and pushed to `ghcr.io/obscura-messaging/obscura-server`.
   - **GitHub Release**: A release is created with generated changelogs and assets.

The release workflows use the repository's `GITHUB_TOKEN`; no deploy key is
needed. The bump workflow also dispatches CI for the updated `main` branch.
If the tag push succeeds but a dispatch fails, rerun the missing CI or **Publish
Release** workflow manually with the appropriate ref. Do not bump the version again.

New GHCR packages default to private even for public repositories. Before switching
deployments to the organization image, make the package public in its GitHub
settings and verify that its versioned and `latest` tags are anonymously pullable.
