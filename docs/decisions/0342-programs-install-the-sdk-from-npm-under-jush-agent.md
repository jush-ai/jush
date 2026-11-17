---
status: proposed
---

# Programs install the SDK from npm, under `@jush-agent`

Supersedes: 0332 (Demi becomes Jush), its packages' names, `@jush-ai/*`,
and that none is published.

## Problem

A program that runs Jush's agents follows the SDK's README, runs
`npm install @jush-ai/sdk zod`, and npm answers 404: no package is
published. Today the program's developer signs in to GitHub, downloads a
build's `sdk` artifact, installs five of its eleven tarballs by hand, the
one of their platform among them, and with Bun also lists each in
`overrides`, since Bun resolves the main package's dependencies from the
registry. Every new build repeats this.

## Decision

The program installs the SDK as any package:

```sh
npm install @jush-agent/sdk zod
```

### The names

Every package of the workspace is `@jush-agent/<name>`, published or not,
so the workspace has one scope: `@jush-agent/web-ui`,
`@jush-agent/plugin-browser` and `@jush-agent/sdk` alike. Nothing stored
holds a package's name: a plugin's page package, which its manifest names,
is built into the same server or core as the page that looks it up.

### What is published

Eight packages, at the build's version (`0.2.<commits>`,
`.github/scripts/version.sh`), under npm's `latest` tag:

| Package | Holds |
| --- | --- |
| `@jush-agent/sdk` | the SDK, its platform packages as optional dependencies |
| `@jush-agent/sdk-<os>-<cpu>`, six | a target's `jush-core`, `jush-runner` and command programs |
| `@jush-agent/web-components` | the conversation views for a program's page (0302) |

A package manager installs only the platform package whose `os` and `cpu`
are the machine's (0301).

`protocol`, `conversation-client` and `utils` are the SDK's own code, which
the web app shares: the build bundles them into the SDK, its code and its
type declarations, so the SDK depends on `zod` alone and they stay
`private`. The web components are one bundle already.

### Publishing

A maintainer pushes a tag that names the SDK and the commit's version on the
commit to publish:

```sh
version=$(.github/scripts/version.sh)
git tag "@jush-agent/sdk@$version"
git push origin "@jush-agent/sdk@$version"
```

The tag starts `.github/workflows/sdk.yml`. GitHub starts a workflow by hand
only when the default branch holds it, and a branch's own workflow is
started by the branch's events. The workflow refuses a tag that names
another version than the commit's, builds the commit by calling `build.yml`,
then publishes the build's `sdk` artifact:

- It signs in by npm's trusted publishing: GitHub's OIDC token of the run,
  which npm accepts from this workflow of this repository only. No token is
  stored, and each version carries its provenance.
- The platform packages go first and the SDK last, so an install never
  meets a platform package that is not published yet.
- A version at or below the SDK's newest is refused, since `latest` would
  move back to it. A package that has the version already, which a run that
  failed midway published, is skipped when npm holds the same file, so the
  failed job runs again. One from another build is refused: a build again is
  not the same bytes, and the SDK's command descriptors name the digests of
  its platform packages' files.

A pushed commit publishes nothing: a published version cannot be taken back,
and each push would leave one.

The version counts the commit's history, so it names a commit along one line
of history only. A branch that `main` does not hold publishes versions that a
later commit of `main` takes too, for its server release: the two `0.2.6246`
are different code until the branch is merged.

### The first version

npm configures a package's trusted publisher only once the package exists,
and the configuration expires unless a run publishes with it within two
days. A maintainer therefore makes each of the eight packages once, on their
own machine signed in with `npm login`: a placeholder `0.0.1`, whose
manifest holds its name alone and whose README says it is one, as
[setup-npm-trusted-publish](https://github.com/azu/setup-npm-trusted-publish)
makes. Then they name the workflow for each:

```sh
npm trust github @jush-agent/sdk --file sdk.yml --repo jush-ai/jush --allow-publish --yes
```

and push the first tag, whose run publishes the first real version.
`--allow-publish` lets the workflow publish directly; without it, npm
allows only `npm stage publish`, whose every version of every package a
maintainer approves with 2FA.

## Spike

A build's tarballs were installed in an empty project with npm and with Bun
(with `overrides`), and a conversation ran a command on a device from the
installed package, on Bun and Node.js. A platform package is 62 to 69 MB
packed; npm documents no limit, and the first version settles it.

tsdown bundled the three packages into the SDK with `deps.alwaysBundle` and
`dts.eager`: the code imports only Node.js's modules and `zod`, and is
499 kB, its declarations 680 kB.

## Prior art

- esbuild, swc and Biome publish one package for each platform as optional
  dependencies of the main one (0301).
- npm's [trusted publishing](https://docs.npmjs.com/trusted-publishers) is
  the OpenSSF's trusted publishers standard, as PyPI and RubyGems have it.
- `release.yml` publishes a build's GitHub release when started by hand on a
  branch; `sdk.yml` is its counterpart for npm.

## Alternatives

- `@jush-ai`, the GitHub organization's name: `@jush-agent` says what the
  packages are.
- Only the published packages under `@jush-agent`: two scopes in one
  workspace, with no rule to tell them apart but publishing.
- Publishing `protocol`, `conversation-client` and `utils` as well: three
  packages whose every export becomes a promise to programs, though only the
  SDK uses them.
- A token in the repository's secrets: it can leak, and npm ends direct
  publishing by a token that bypasses 2FA in January 2027.
- Publishing every push of a branch: see Publishing.

## Open questions

None.
