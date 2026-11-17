#!/usr/bin/env bash
# Publishes the SDK's packed packages in DIRECTORY to npm, all at one version:
# the SDK last, so an install never meets a platform package that is not
# published yet.
#
# Usage: publish-sdk.sh DIRECTORY
#
# A version below or at the SDK's newest is refused, since npm's `latest`
# would move back to it, or it is published already. A package that has the
# version already, which a failed run published, is skipped when npm holds
# this very file, as a re-run of the failed job finds; a file of another
# build is refused, since a build again is not the same bytes, and the SDK's
# command descriptors name the digests of the files in this build's platform
# packages. npm signs in by the run's OIDC token (trusted publishing).

set -euo pipefail

readonly sdk=@jush-agent/sdk

# Prints the name and version of the packed package FILE, one a line.
manifest() {
  tar -xzOf "$1" package/package.json | jq -r '.name, .version'
}

# Prints FIELD of what npm has of SPEC, such as the `version` of a package's
# newest or the `dist.integrity` of one of its versions, or nothing when it
# has none. Fails when npm does not answer, so a registry that is down is
# never taken for one without the package.
published() {
  local answer
  if answer=$(npm view "$1" "$2" --json 2>&1); then
    jq -r '.' <<< "$answer"
    return
  fi
  grep -q 'E404' <<< "$answer" || { echo "$answer" >&2; return 1; }
}

main() {
  local directory=$1 file name version sdk_file="" newest integrity
  local -a others=()
  for file in "$directory"/*.tgz; do
    { read -r name; read -r version; } < <(manifest "$file")
    if [[ "$name" == "$sdk" ]]; then
      sdk_file=$file
    else
      others+=("$file")
    fi
  done
  [[ -n "$sdk_file" ]] || { echo "$directory holds no $sdk" >&2; exit 1; }
  { read -r name; read -r version; } < <(manifest "$sdk_file")
  newest=$(published "$sdk" version)
  if [[ -n "$newest" && "$(printf '%s\n%s\n' "$newest" "$version" | sort -V | tail -n 1)" == "$newest" ]]; then
    echo "$sdk@$version is not above $newest, the newest published" >&2
    exit 1
  fi
  for file in "${others[@]}" "$sdk_file"; do
    { read -r name; read -r version; } < <(manifest "$file")
    integrity=$(published "$name@$version" dist.integrity)
    if [[ -n "$integrity" ]]; then
      if [[ "$integrity" != "sha512-$(openssl dgst -sha512 -binary "$file" | base64 | tr -d '\n')" ]]; then
        echo "$name@$version is published from another build: publish a later commit" >&2
        exit 1
      fi
      echo "$name@$version is published already"
      continue
    fi
    npm publish "$file" --access public
  done
}

main "$@"
