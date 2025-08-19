# AGENTS

## Scope
These instructions apply to the entire repository.

## Search
- Use `rg` (ripgrep) for code search.
- Avoid `grep -R` and similar recursive search commands.

## Code Style
- Format Rust code with `cargo fmt --all` before committing.

## Linting
- Run `cargo clippy -- -D warnings` and address any warnings.

## Tests
- Run `cargo test --workspace` from the repository root.

## Commit Messages
- Follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `chore:`, etc.).

## Pull Requests
- Summarize changes and include test results in the PR description.
