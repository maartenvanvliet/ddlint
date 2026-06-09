//! Per-dialect rule registries.
//!
//! Each `<dialect>_rules()` function returns the list of rules for that
//! dialect. Rules are plain structs from `crate::rules` configured with
//! dialect-specific severity and detail text.
//!
//! To add a new dialect:
//!   1. Add a variant to [`crate::dialect::Dialect`].
//!   2. Write a `<dialect>_rules()` function here.
//!   3. Add a match arm in [`crate::dialect::Dialect::rules`].

use crate::finding::Severity;
use crate::rules::{
    AddColumnEnumRule, AddColumnNoAlgorithmInstantRule, AddColumnNotNullNoDefaultRule,
    AddForeignKeyRule, AddPrimaryKeyRule, AddUniqueConstraintRule, AlterForeignKeyRule,
    ChangeColumnEnumRule, ChangeColumnRule, CreateUniqueIndexRule, DialectRule, DropColumnRule,
    DropPrimaryKeyRule, DropTableRule, FileRule, LockTablesRule, ModifyColumnEnumRule,
    ModifyColumnRule, MultiStatementMigrationRule, RenameColumnRule, RenameTableRule, TruncateRule,
};

pub fn mysql_rules() -> Vec<Box<dyn DialectRule>> {
    vec![
        Box::new(AddColumnNotNullNoDefaultRule {
            severity: Severity::Danger,
            detail: "Adding a NOT NULL column without a DEFAULT requires MySQL to \
                     verify or backfill every existing row before the migration \
                     completes. On MySQL < 8.0 this takes an ACCESS EXCLUSIVE lock \
                     for the full duration — all reads and writes are blocked. On \
                     MySQL 8.0+ it runs INPLACE but is still slow on large tables.\n\
                     \n\
                     Fix: add a DEFAULT value (e.g. DEFAULT '' or DEFAULT 0) so \
                     MySQL can apply the change instantly, then tighten the \
                     constraint in a separate migration once the column is populated.",
        }),
        Box::new(AddColumnNoAlgorithmInstantRule {
            severity: Severity::Warning,
            detail: "MySQL 8.0+ supports ALGORITHM=INSTANT for adding columns, which \
                     completes in milliseconds regardless of table size by only updating \
                     the data dictionary — no table rebuild or row scanning required.\n\
                     \n\
                     Without ALGORITHM=INSTANT, MySQL chooses the algorithm \
                     automatically. It will often pick INPLACE or COPY, both of which \
                     read the entire table and hold locks for the duration.\n\
                     \n\
                     Fix: append ALGORITHM=INSTANT to the statement:\n\
                     ALTER TABLE t ADD COLUMN x TEXT, ALGORITHM=INSTANT;\n\
                     If MySQL rejects it (e.g. the column has a non-null default on \
                     MySQL < 8.0), the statement will fail fast rather than silently \
                     running a slow rebuild.",
        }),
        Box::new(AddColumnEnumRule {
            severity: Severity::Warning,
            detail: "ENUM columns always use ALGORITHM=COPY in MySQL, which rebuilds \
                     the entire table and holds a write lock for the duration. This \
                     applies even when adding a new ENUM column, not just when \
                     modifying an existing one.\n\
                     \n\
                     Fix: use VARCHAR with an application-level or CHECK constraint \
                     instead, or add a separate lookup table with a foreign key. \
                     Either approach avoids the table rebuild.",
        }),
        Box::new(ModifyColumnRule {
            severity: Severity::Danger,
            detail: "MODIFY COLUMN changes the column definition in-place when MySQL \
                     determines the storage format is compatible (ALGORITHM=INPLACE), \
                     but falls back to a full table rebuild (ALGORITHM=COPY) for many \
                     common changes: type changes, charset changes, changing nullability \
                     without a DEFAULT, or reordering columns.\n\
                     \n\
                     A COPY rebuild holds a write lock for the entire duration and \
                     creates a full copy of the table on disk — dangerous for any table \
                     over a few hundred MB.\n\
                     \n\
                     Fix: add ALGORITHM=INPLACE, LOCK=NONE to fail fast if MySQL would \
                     fall back to COPY. For changes that genuinely require COPY, use \
                     pt-online-schema-change or gh-ost to do the rebuild online.",
        }),
        Box::new(ModifyColumnEnumRule {
            severity: Severity::Danger,
            detail: "Any modification to an ENUM column forces ALGORITHM=COPY \
                     regardless of what changed. Even adding a new valid value \
                     triggers a full table rebuild with a write lock.\n\
                     \n\
                     Fix: migrate ENUM columns to VARCHAR. Use pt-online-schema-change \
                     to do the initial conversion without downtime.",
        }),
        Box::new(ChangeColumnRule {
            severity: Severity::Danger,
            detail: |old, new| format!(
                "CHANGE COLUMN renames `{old}` to `{new}`. Any app code \
                 still using the old column name will fail immediately after the \
                 migration runs — before the new deployment is complete. In a \
                 rolling deploy this breaks in-flight requests and old pods.\n\
                 \n\
                 Fix: use the expand-contract (parallel change) pattern: \
                 (1) add `{new}` as a nullable column, \
                 (2) dual-write to both columns in the application, \
                 (3) backfill `{new}` from `{old}`, \
                 (4) switch reads to `{new}`, \
                 (5) drop `{old}` in a later migration."
            ),
        }),
        Box::new(ChangeColumnEnumRule {
            severity: Severity::Danger,
            detail: "ENUM columns always force a full table rebuild. See CHANGE_COLUMN and ADD_COLUMN_ENUM findings.",
        }),
        Box::new(RenameColumnRule {
            severity: Severity::Danger,
            detail: |old, new| format!(
                "Renaming `{old}` to `{new}` is atomic in \
                 MySQL 8.0+ (ALGORITHM=INSTANT) so it won't lock, but it immediately \
                 breaks any live app code still reading `{old}`. In a \
                 rolling deploy, old pods will start failing before new pods are up.\n\
                 \n\
                 Fix: use the expand-contract pattern — add the new column name, \
                 dual-write, backfill, switch reads, then drop the old column."
            ),
        }),
        Box::new(RenameTableRule {
            severity: Severity::Danger,
            detail: |old, new| format!(
                "Renaming `{old}` to `{new}` breaks all live app code \
                 referencing the old table name immediately. Foreign keys pointing \
                 to `{old}` are also affected.\n\
                 \n\
                 Fix: if this is a permanent rename, use the expand-contract \
                 pattern with views as a compatibility shim: create `{new}`, \
                 replace `{old}` with a view, migrate the application, then \
                 drop the view."
            ),
        }),
        Box::new(DropColumnRule {
            severity: Severity::Danger,
            detail: |col| format!(
                "Dropping `{col}` is instant in MySQL 8.0 (ALGORITHM=INSTANT) \
                 but permanently destroys the data and breaks any live app code still \
                 reading or writing it. In a rolling deploy, old pods will crash \
                 immediately.\n\
                 \n\
                 Fix: ensure all application code has been deployed without any \
                 reference to `{col}` before running this migration. The \
                 column should have been ignored by the ORM/queries for at least \
                 one full deploy cycle."
            ),
        }),
        Box::new(AddPrimaryKeyRule {
            severity: Severity::Danger,
            detail: "InnoDB tables are organised as a B-tree clustered on the primary key. \
                     Adding a primary key to a table that lacks one (or replacing one) requires \
                     MySQL to physically rewrite every row in primary-key order — this is always \
                     ALGORITHM=COPY with a full write lock for the entire duration.\n\
                     \n\
                     Fix: use pt-online-schema-change or gh-ost to add the primary key \
                     without downtime. On MySQL 8.0 you can add an invisible PK first \
                     (if the table has a generated row ID), then make it visible in a \
                     separate fast step.",
        }),
        Box::new(DropPrimaryKeyRule {
            severity: Severity::Danger,
            detail: "Dropping the primary key requires ALGORITHM=COPY — a full table \
                     rebuild with a write lock. InnoDB tables are clustered on the \
                     primary key, so removing it forces a complete reorganisation of \
                     the on-disk data structure.\n\
                     \n\
                     Fix: use pt-online-schema-change or gh-ost. Avoid dropping \
                     primary keys on large tables if at all possible.",
        }),
        Box::new(AddForeignKeyRule {
            severity: Severity::Danger,
            detail: |table| format!(
                "Adding a foreign key causes MySQL to validate that every \
                 existing row in `{table}` satisfies the constraint. During \
                 this scan MySQL holds a metadata lock that blocks all DDL \
                 on both the referencing and referenced tables. On large \
                 tables this can take minutes.\n\
                 \n\
                 Fix: (1) add the supporting index as a separate migration \
                 first (CREATE INDEX is online), (2) only add the FK \
                 constraint once the index exists, preferably during a \
                 low-traffic window. If referential integrity can be enforced \
                 at the application layer, consider omitting the FK entirely."
            ),
        }),
        Box::new(AddUniqueConstraintRule {
            severity: Severity::Warning,
            detail: "Building the unique index requires MySQL to read and sort \
                     the entire table to check for duplicates. This is done \
                     online (reads allowed) but writes are blocked once the \
                     index build finishes and the lock is promoted. The migration \
                     will also fail outright if any duplicate values exist.\n\
                     \n\
                     Fix: check for duplicates before running this migration \
                     (SELECT col, COUNT(*) ... GROUP BY ... HAVING COUNT(*) > 1). \
                     Run during a low-traffic window on large tables.",
        }),
        Box::new(CreateUniqueIndexRule {
            severity: Severity::Warning,
            detail: "Building a unique index requires a full table read to verify there are \
                     no duplicate values. The index build itself is online, but the migration \
                     will fail if duplicates exist at the time it runs.\n\
                     \n\
                     Fix: run SELECT COUNT(*) vs SELECT COUNT(DISTINCT col) first to confirm \
                     uniqueness. If cleaning up duplicates, do that in a prior migration.",
        }),
        Box::new(DropTableRule {
            severity: Severity::Danger,
            detail: |name| format!(
                "Dropping `{name}` permanently destroys all data and the table definition. \
                 Any live app code referencing `{name}` will immediately start throwing \
                 errors. In a rolling deploy this affects old pods that haven't been \
                 restarted yet.\n\
                 \n\
                 Fix: ensure all application code referencing `{name}` has been removed \
                 and fully deployed before running this migration. Consider renaming the \
                 table first and waiting one deploy cycle to confirm nothing breaks."
            ),
        }),
        Box::new(TruncateRule {
            severity: Severity::Danger,
            detail: |name| format!(
                "TRUNCATE deletes every row from `{name}` and acquires a metadata lock \
                 for the duration. Unlike DELETE it cannot be rolled back in the same \
                 transaction in MySQL (it causes an implicit commit).\n\
                 \n\
                 This is almost always a mistake in a schema migration. If you need to \
                 clear data as part of a migration, use DELETE with a WHERE clause so \
                 the operation is scoped and transactional."
            ),
        }),
        Box::new(LockTablesRule {
            severity: Severity::Danger,
            detail: "LOCK TABLES acquires an explicit lock on each named table, blocking \
                     every other connection from reading (READ lock) or reading and writing \
                     (WRITE lock) until UNLOCK TABLES is called. In a migration script this \
                     means all application traffic is frozen for the duration of the lock.\n\
                     \n\
                     LOCK TABLES is almost never needed in a migration — DDL statements \
                     acquire their own internal locks automatically. If you're using it to \
                     serialise access during a data copy, use SELECT ... FOR UPDATE on \
                     individual rows instead, or use pt-online-schema-change which handles \
                     locking safely.",
        }),
    ]
}

pub fn mysql_file_rules() -> Vec<Box<dyn FileRule>> {
    vec![
        Box::new(MultiStatementMigrationRule {
            severity: Severity::Warning,
            detail: "MySQL has no transactional DDL. Each DDL statement causes an implicit \
                     commit before it executes, so multiple DDL statements in one migration \
                     file are not atomic.\n\
                     \n\
                     If the second statement fails, the first is already committed and \
                     cannot be rolled back. Tools like Flyway and Liquibase will mark the \
                     migration as failed, leaving the schema in a partial state that requires \
                     manual intervention to repair.\n\
                     \n\
                     Fix: split this file into one DDL statement per migration file. \
                     Each file then succeeds or fails atomically from the migration \
                     tool's perspective.",
        }),
        Box::new(AlterForeignKeyRule {
        severity: Severity::Danger,
        detail: "Altering a foreign key in MySQL requires dropping and re-adding the constraint. \
                 When MySQL adds the new FK it validates every existing row against the constraint, \
                 acquiring a metadata lock that blocks all DDL on both tables for the duration.\n\
                 \n\
                 Without SET FOREIGN_KEY_CHECKS = 0, this validation runs synchronously and \
                 can take minutes on large tables. The lock also prevents the UNLOCK from the \
                 other end of a rolling deploy from proceeding.\n\
                 \n\
                 Fix: wrap the DROP + ADD pair with:\n\
                 SET FOREIGN_KEY_CHECKS = 0;\n\
                 ALTER TABLE ... DROP FOREIGN KEY ...;\n\
                 ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY ...;\n\
                 SET FOREIGN_KEY_CHECKS = 1;\n\
                 \n\
                 This skips row-level validation. Only safe if you are certain the existing \
                 data already satisfies the new constraint (e.g. only changing ON DELETE \
                 behaviour, not the referenced column or table).",
        }),
    ]
}
