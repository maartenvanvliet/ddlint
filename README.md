# ddlint

Lint SQL migration files for zero-downtime safety.

Parses each migration and flags DDL statements that risk table locks, downtime,
or data loss during a rolling deploy.

## Installation

### Pre-built binaries

Download the latest binary for your platform from the
[Releases](https://github.com/maartenvanvliet/ddlint/releases/latest) page and
place it on your `PATH`.

Or install in one line:

```sh
# Linux (x86_64)
curl -sSL https://github.com/maartenvanvliet/ddlint/releases/latest/download/ddlint-$(curl -sSL https://api.github.com/repos/maartenvanvliet/ddlint/releases/latest | grep tag_name | cut -d'"' -f4)-x86_64-unknown-linux-musl.tar.gz \
  | tar -xz -C /usr/local/bin

# macOS (Apple Silicon)
curl -sSL https://github.com/maartenvanvliet/ddlint/releases/latest/download/ddlint-$(curl -sSL https://api.github.com/repos/maartenvanvliet/ddlint/releases/latest | grep tag_name | cut -d'"' -f4)-aarch64-apple-darwin.tar.gz \
  | tar -xz -C /usr/local/bin

# macOS (Intel)
curl -sSL https://github.com/maartenvanvliet/ddlint/releases/latest/download/ddlint-$(curl -sSL https://api.github.com/repos/maartenvanvliet/ddlint/releases/latest | grep tag_name | cut -d'"' -f4)-x86_64-apple-darwin.tar.gz \
  | tar -xz -C /usr/local/bin
```

### Build from source

Requires [Rust](https://rustup.rs/) 1.70 or later.

```sh
git clone https://github.com/maartenvanvliet/ddlint.git
cd ddlint
cargo build --release
# binary is at target/release/ddlint
```

To install directly into `~/.cargo/bin`:

```sh
cargo install --path .
```

## Usage

```
ddlint [OPTIONS] <INPUT>...
```

Inputs can be any mix of files, directories (walked recursively for `.sql`
files), or quoted glob patterns:

```sh
ddlint migrations/
ddlint migrations/V1__init.sql migrations/V2__add_index.sql
ddlint 'migrations/V*.sql'
ddlint --config ddlint.yml migrations/
```

### Options

| Flag | Description |
|------|-------------|
| `-c, --config <FILE>` | Path to a config file. Defaults to `ddlint.yml` in the current directory. |
| `-f, --format <FORMAT>` | Output format: `text` (default) or `gha` (GitHub Actions annotations). |
| `--dialect <ENGINE>` | SQL dialect / database engine (e.g. `mysql`). Overrides the config file. |
| `--strict` | Treat warnings as errors (exit 1 if any warnings are present). |
| `--print-config` | Print the default configuration YAML for the active dialect and exit. |

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | All migrations clean. |
| `1` | Findings present. |
| `2` | Bad arguments, missing files, or config error. |

## Rules

| Rule | Default | Description |
|------|---------|-------------|
| `ADD_COLUMN_NOT_NULL_NO_DEFAULT` | danger | NOT NULL column with no DEFAULT forces a full backfill. |
| `ADD_COLUMN_NO_ALGORITHM_INSTANT` | warning | ADD COLUMN without `ALGORITHM=INSTANT` may trigger a slow rebuild. |
| `ADD_COLUMN_ENUM` | warning | ENUM columns always use `ALGORITHM=COPY`. |
| `MODIFY_COLUMN` | danger | May trigger a full table rebuild. |
| `MODIFY_COLUMN_ENUM` | danger | ENUM modification always uses `ALGORITHM=COPY`. |
| `CHANGE_COLUMN` | danger | Renames a column, breaking live app code immediately. |
| `CHANGE_COLUMN_ENUM` | danger | ENUM + rename always forces `ALGORITHM=COPY`. |
| `RENAME_COLUMN` | danger | Breaks live app code referencing the old name. |
| `RENAME_TABLE` | danger | Breaks all live references to the old table name. |
| `DROP_COLUMN` | danger | Irreversible; breaks live code in a rolling deploy. |
| `ADD_PRIMARY_KEY` | danger | Requires a full table rebuild (`ALGORITHM=COPY`). |
| `DROP_PRIMARY_KEY` | danger | Requires `ALGORITHM=COPY` — full table rebuild. |
| `ADD_FOREIGN_KEY` | danger | Acquires a metadata lock during constraint validation. |
| `ADD_UNIQUE_CONSTRAINT` | warning | Requires a full duplicate scan; writes blocked at promotion. |
| `CREATE_UNIQUE_INDEX` | warning | Full table read to verify uniqueness. |
| `DROP_TABLE` | danger | Irreversible; destroys data and breaks live references. |
| `TRUNCATE` | danger | Destroys all rows; implicit commit prevents rollback. |
| `LOCK_TABLES` | danger | Blocks all application traffic for the duration of the lock. |

## Configuration

Generate a starter config for the active dialect:

```sh
ddlint --print-config > ddlint.yml
```

Place a `ddlint.yml` in your project root to override rule severities:

```yaml
rules:
  MODIFY_COLUMN: warn      # downgrade from danger
  ADD_COLUMN_ENUM: ignore  # suppress entirely
  CREATE_UNIQUE_INDEX: error  # upgrade from warning
```

Valid levels: `error` / `danger`, `warn` / `warning`, `ignore` / `off`.

## GitHub Actions

```yaml
# .github/workflows/ddlint.yml
name: Migration Lint

on:
  push:
    branches: [main]
    paths:
      - 'migrations/**/*.sql'
  pull_request:
    paths:
      - 'migrations/**/*.sql'

jobs:
  lint:
    name: Lint migrations
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install ddlint
        run: |
          VERSION=$(curl -sSL https://api.github.com/repos/maartenvanvliet/ddlint/releases/latest | grep tag_name | cut -d'"' -f4)
          curl -sSL "https://github.com/maartenvanvliet/ddlint/releases/download/${VERSION}/ddlint-${VERSION}-x86_64-unknown-linux-musl.tar.gz" \
            | tar -xz -C /usr/local/bin

      - name: Run migration linter
        run: ddlint --format gha migrations/
```

The `--format gha` flag emits `::error` and `::warning` workflow commands,
which GitHub renders as inline annotations on PR diffs. Add `--strict` to fail
the job on warnings as well as errors.
