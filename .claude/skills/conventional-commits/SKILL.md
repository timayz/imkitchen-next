---
name: conventional-commits
description: Enforce the Conventional Commits 1.0.0 specification for all git commit messages. Use whenever writing, editing, reviewing, or generating commit messages, or when configuring commit-related tooling (commitlint, husky hooks, release-please, semantic-release, changelog generation). Covers the `<type>[scope]: <description>` header format, the allowed type vocabulary (feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert), scope conventions, breaking-change markers (`!` and `BREAKING CHANGE:` footer), body and footer rules, multi-paragraph bodies, multiple footers, revert commits, and the SemVer mapping (feat → MINOR, fix → PATCH, BREAKING CHANGE → MAJOR). Also flags common anti-patterns: vague subjects ("update", "fix stuff"), missing type, capitalised/punctuated subjects, mixing unrelated changes in one commit, and `BREAKING CHANGE` written in the subject instead of as a footer or `!` marker.
disable-model-invocation: true
---

# Conventional Commits — Commit Message Convention

All commits in this repository MUST follow the [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) specification. This produces a machine-readable commit history that drives changelog generation, semantic version bumps, and release automation.

## Format

```
<type>[optional scope][!]: <description>

[optional body]

[optional footer(s)]
```

- **Header line is mandatory.** Body and footers are optional.
- A blank line separates header, body, and footers.
- The whole header should fit within ~72 characters; hard-wrap body at ~72 columns.

### Minimal example

```
fix: prevent panic when config file is missing
```

### With scope

```
feat(auth): add OAuth2 PKCE flow
```

### With body and footer

```
refactor(parser): replace recursive descent with Pratt parser

The recursive descent implementation hit stack overflow on deeply
nested expressions. Pratt parsing is iterative and handles arbitrary
nesting depth.

Closes #482
Reviewed-by: alice@example.com
```

## Types

Use exactly one of these in the header. They are lowercase and have fixed meanings:

| Type       | Meaning                                                              | SemVer impact |
| ---------- | -------------------------------------------------------------------- | ------------- |
| `feat`     | New user-visible feature                                             | MINOR         |
| `fix`      | Bug fix                                                              | PATCH         |
| `docs`     | Documentation only                                                   | none          |
| `style`    | Formatting, whitespace, missing semicolons — no code-behavior change | none          |
| `refactor` | Code change that neither fixes a bug nor adds a feature              | none          |
| `perf`     | Performance improvement                                              | PATCH         |
| `test`     | Adding or correcting tests                                           | none          |
| `build`    | Build system, dependencies, package manifests                        | none          |
| `ci`       | CI configuration (GitHub Actions, etc.)                              | none          |
| `chore`    | Routine maintenance not covered above (release commits, tooling)     | none          |
| `revert`   | Reverts a previous commit (see below)                                | depends       |

A `BREAKING CHANGE` overrides the above and forces a MAJOR bump regardless of type.

## Scope

The scope is an optional noun in parentheses naming the area of code affected:

```
feat(api): ...
fix(parser): ...
docs(readme): ...
```

Guidelines:
- Lowercase, single word or kebab-case.
- Use a stable vocabulary — prefer existing scopes already in `git log` over inventing new ones.
- Omit the scope when the change is genuinely repo-wide (e.g. `build: bump rust edition to 2024`).

## Description

The text after `: ` on the header line.

- **Lowercase** first letter. (`fix: handle empty input`, not `fix: Handle empty input`.)
- **Imperative mood**, present tense. (`add`, not `added` or `adds`.)
- **No trailing period.**
- Describe *what changes for the user/reader*, not *how* you implemented it.

```
# GOOD
feat(cli): support --json output flag
fix(db): close connection pool on shutdown

# BAD
feat(cli): Added a new flag for json output.   # capitalised + past tense + period
fix: stuff                                     # vague
update code                                    # missing type, vague
```

## Body

Optional. Separated from the header by **one blank line**. Use it to explain the *why* and any non-obvious context — the *what* is in the diff.

- Free-form prose, wrapped at ~72 columns.
- May contain multiple paragraphs separated by blank lines.
- Reference issues, prior commits, or design docs as needed.

## Footers

Optional. Separated from the body by **one blank line**. Each footer is a `Token: value` pair on its own line, following the [git trailer](https://git-scm.com/docs/git-interpret-trailers) format.

```
Closes #123
Refs #456, #457
Reviewed-by: alice@example.com
Co-authored-by: bob <bob@example.com>
```

Token rules:
- Tokens use `-` instead of spaces (`Reviewed-by`, not `Reviewed by`) — **with one exception:** `BREAKING CHANGE` is written with a space, in uppercase.
- Multiple footers are allowed; one per line.

## Breaking changes

A breaking change MUST be signalled in **one** of two ways (both is also fine):

### 1. `!` before the colon in the header

```
feat(api)!: remove deprecated /v1 endpoints
refactor!: drop support for Node 16
```

### 2. `BREAKING CHANGE:` footer

```
feat(api): migrate auth tokens to JWT

BREAKING CHANGE: clients holding session cookies issued before this
release must re-authenticate. The /auth/refresh endpoint no longer
accepts the legacy cookie format.
```

When both are used, the footer provides the detail; the `!` makes the breakage scannable in `git log --oneline`.

- A breaking change forces a **MAJOR** SemVer bump regardless of type.
- Even a `fix!:` or `chore!:` counts as breaking.
- Do **not** write `BREAKING CHANGE` in the header description — it belongs in the footer or signalled by `!`.

## Revert commits

When reverting a previous commit, use the `revert` type and reference the reverted SHA(s) in the footer:

```
revert: feat(auth): add OAuth2 PKCE flow

This reverts commit 7f3c2a9b8d1e4f5a6c7b8d9e0f1a2b3c4d5e6f7a.

Refs #602
```

The header description is conventionally the header of the commit being reverted.

## Multiple changes in one commit

Don't. Split unrelated changes into separate commits, each with its own conventional header. If a single logical change touches multiple areas, pick the dominant scope (or omit scope) and describe the change holistically in the body.

## Common anti-patterns to flag

When reviewing or generating commit messages, reject these:

| Anti-pattern                                          | Fix                                                          |
| ----------------------------------------------------- | ------------------------------------------------------------ |
| `update code`, `fix stuff`, `wip`, `asdf`             | Use a real type and a specific description.                  |
| `Fix: Add login` (capitalised type or description)    | Lowercase both: `fix: add login`.                            |
| `feat: added new endpoint.` (past tense + period)     | `feat: add new endpoint`                                     |
| `BREAKING CHANGE: drop v1` in the subject line        | Move to footer, or use `feat!:` / `fix!:` in the header.     |
| Multi-line subject                                    | Keep the header on one line; move detail to the body.        |
| Missing blank line before body or footer              | Insert blank line — parsers depend on it.                    |
| `feat(All): …` — overly broad or made-up scope        | Drop the scope or use a recognised one.                      |
| Mixing a refactor and a fix in one commit             | Split into two commits.                                      |

## Quick checklist when writing a commit

1. Pick the **type** (`feat`, `fix`, `docs`, …).
2. Pick a **scope** if there's a clear one; otherwise omit.
3. Write the **description**: lowercase, imperative, no period, ≤ ~50 chars ideal.
4. If breaking: add `!` before `:` and/or a `BREAKING CHANGE:` footer.
5. Add a **body** if the *why* isn't obvious from the diff.
6. Add **footers** for issue refs, co-authors, reviewers, breaking-change detail.
7. Verify the header still fits on one line under ~72 chars.
