#!/usr/bin/env bash
# Usage: publish-release.sh FILES_DIR NOTES_FILE
#
# Makes the files in FILES_DIR (package-release.sh's output) the assets of
# the pre-release tagged main-latest, and moves that tag to this commit. One
# release is kept and updated, not one per push: main is built often, and
# the README links to releases/download/main-latest/<name>. A pre-release
# that is not "latest", so it never stands in for a numbered release.
# Run by the workflow's release job with GH_TOKEN, GITHUB_REPOSITORY and
# GITHUB_SHA set.
set -euo pipefail
dir=$1
notes=$(realpath "$2")
tag=main-latest
repo=$GITHUB_REPOSITORY
sha=$GITHUB_SHA
title="BlackCAD, latest main (${sha::7})"
summary=${GITHUB_STEP_SUMMARY:-/dev/null}

cd "$dir"
sha256sum --check SHA256SUMS.txt

# Runs finish in any order, so a slow older run must not put its build over
# a newer one. Publish only when the commit the tag is on now is this
# commit or an ancestor of it. No tag yet (or no answer) means publish.
if current=$(gh api "repos/$repo/commits/$tag" --jq .sha 2>/dev/null); then
  status=$(gh api "repos/$repo/compare/$current...$sha" --jq .status)
  case "$status" in
    ahead | identical) ;;
    *)
      echo "$tag is already at ${current::7} and ${sha::7} is not ahead of it (compare: $status): leaving it."
      echo "### Skipped: \`$tag\` is already at \`${current::7}\`, and \`${sha::7}\` is not ahead of it (compare: $status)" >>"$summary"
      exit 0
      ;;
  esac
fi

if gh release view "$tag" >/dev/null 2>&1; then
  # Upload over the old files first and move the tag after, so the page
  # never lists fewer files than it did. Then drop what is no longer built.
  gh release upload "$tag" --clobber -- *
  gh release edit "$tag" --title "$title" --notes-file "$notes" --prerelease --latest=false
  gh release view "$tag" --json assets --jq '.assets[].name' | while IFS= read -r name; do
    [ -e "$name" ] || gh release delete-asset "$tag" "$name" --yes
  done
else
  gh release create "$tag" --prerelease --latest=false --target "$sha" \
    --title "$title" --notes-file "$notes" -- *
fi
# A new release made the tag on this commit already; an old one is moved.
gh api -X PATCH "repos/$repo/git/refs/tags/$tag" -f sha="$sha" -F force=true --silent

{
  echo "### Published \`$tag\` at \`${sha::7}\`"
  echo
  sed 's/^/    /' SHA256SUMS.txt
} >>"$summary"
