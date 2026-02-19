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
2. **Commit & Tag**: Commit the version change and create a git tag for the release.
3. **Publish**: Trigger the **Publish Release** workflow to build and publish artifacts:
   - **Crates.io**: The updated crate is published.
   - **GHCR (GitHub Container Registry)**: A new Docker image is built and pushed.
   - **GitHub Release**: A release is created with generated changelogs and assets.
