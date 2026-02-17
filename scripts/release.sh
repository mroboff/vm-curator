#!/usr/bin/env bash
set -euo pipefail

# Release script for vm-curator
# Usage: ./scripts/release.sh <version>
# Example: ./scripts/release.sh 0.5.0

if [ $# -ne 1 ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.5.0"
    exit 1
fi

VERSION="$1"
TAG="v${VERSION}"

# Validate version format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in semver format (e.g., 0.5.0)"
    exit 1
fi

# Ensure working directory is clean
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Error: Working directory has uncommitted changes. Commit or stash them first."
    exit 1
fi

# Ensure we're on main
BRANCH=$(git branch --show-current)
if [ "$BRANCH" != "main" ]; then
    echo "Error: Must be on main branch (currently on '$BRANCH')"
    exit 1
fi

# Check tag doesn't already exist
if git tag -l "$TAG" | grep -q "$TAG"; then
    echo "Error: Tag $TAG already exists"
    exit 1
fi

echo "Releasing vm-curator $VERSION..."

# 1. Update version in Cargo.toml
CURRENT=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "  Bumping version: $CURRENT -> $VERSION"
sed -i "0,/^version = \"$CURRENT\"/s//version = \"$VERSION\"/" Cargo.toml

# 2. Regenerate Cargo.lock
echo "  Regenerating Cargo.lock..."
cargo generate-lockfile

# 3. Verify lockfile matches
echo "  Verifying Cargo.lock consistency..."
cargo fetch --locked

# 4. Run tests
echo "  Running tests..."
cargo test --locked

# 5. Commit both files together
echo "  Committing..."
git add Cargo.toml Cargo.lock
git commit -m "Release $TAG"

# 6. Tag
echo "  Tagging $TAG..."
git tag "$TAG"

# 7. Push commit and tag
echo "  Pushing..."
git push origin main
git push origin "$TAG"

echo ""
echo "Release $TAG pushed successfully!"
echo "CI will build and publish the release at:"
echo "  https://github.com/mroboff/vm-curator/actions"
