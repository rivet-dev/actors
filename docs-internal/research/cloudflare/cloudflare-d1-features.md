# Cloudflare D1 Feature Reference

> **As of:** 2026-03-06
>
> **Cloudflare basis:** Official D1 docs accessed 2026-03-06. D1 is documented as a live platform surface rather than a semver'd SDK.
>
> **Rivet basis:** RivetKit 2.1.5, repo `ba46891b1`, canonical docs under `https://rivet.dev/docs/...`.
>
> **Migration framing:** D1 is a shared managed SQLite service. Rivet's native SQLite is **per actor instance**, so the first migration decision is whether each D1 database becomes one actor, one actor shard, or an external shared SQL service such as PostgreSQL.
>
> **Status legend:** `native` = first-class Rivet feature, `partial` = supported with material semantic gaps, `pattern` = implemented as an application pattern on top of Rivet, `external` = requires a non-Rivet dependency/service, `unsupported` = no acceptable Rivet equivalent today, `out-of-scope` = operational/platform concern outside the Rivet Actor runtime.

## Migration Matrix

| Feature | Description | Status | Confidence | Rivet source | Validation proof | Risk | Notes |
|---------|-------------|--------|------------|--------------|------------------|------|-------|
| Database Creation and Management | Create, list, delete, and inspect D1 databases via CLI or REST API | pattern | medium | [SQLite](https://rivet.dev/docs/actors/sqlite), [Actor Keys](https://rivet.dev/docs/actors/keys) | [examples/sqlite-raw/src/registry.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sqlite-raw/src/registry.ts) | High | Actor creation can stand in for database creation, but this is a topology redesign, not an operational match. |
| Bindings and Configuration | Connect D1 databases to Workers via Wrangler binding declarations | partial | high | [SQLite](https://rivet.dev/docs/actors/sqlite), [PostgreSQL](https://rivet.dev/docs/actors/postgres) | [examples/cloudflare-workers/wrangler.json](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/wrangler.json) | Medium | Native actor SQLite needs no binding. Shared external SQL uses normal env/config, not a D1 binding primitive. |
| Worker Binding API: D1Database Methods | `prepare`, `batch`, `exec`, `dump`, and session methods on the D1Database object | partial | medium | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Medium | `c.db.execute(...)` and Drizzle cover common query paths, but `dump()` and session bookmarks do not exist. |
| Prepared Statement Methods | `bind`, `run`, `raw`, and `first` methods for parameterized query execution | partial | high | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Low | Parameterized SQL is native; the exact API shape differs. |
| Return Objects | `D1Result` and `D1ExecResult` structures with metadata and query results | partial | medium | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Low | Rivet returns row arrays rather than D1's metadata envelope by default. |
| SQL Query Execution via Wrangler | Execute SQL commands or files directly against databases via CLI | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Gap | Low | Rivet does not provide a D1-style SQL CLI for actor-local SQLite. Use app-defined admin actions or external DB tooling. |
| Complete Worker Example | Full JavaScript and TypeScript Worker examples querying D1 | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers), [SQLite](https://rivet.dev/docs/actors/sqlite) | [examples/cloudflare-workers](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/cloudflare-workers), [examples/sqlite-raw](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/sqlite-raw) | Medium | End-to-end examples exist for Workers plus actor-local SQLite. |
| Migrations | Versioned SQL migration files with create, list, and apply workflows | native | high | [SQLite](https://rivet.dev/docs/actors/sqlite), [SQLite + Drizzle](https://rivet.dev/docs/actors/sqlite-drizzle) | [examples/sqlite-drizzle](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/sqlite-drizzle) | Medium | Use `db({ onMigrate })` or Drizzle migrations inside the actor. |
| Time Travel (Point-in-Time Recovery) | Restore databases to any minute within the last 30 days | unsupported | high | [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | High | No documented PITR equivalent for actor-local SQLite. |
| Legacy Backups | Snapshot-based backups for alpha databases, being deprecated | unsupported | high | [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | Medium | No actor-local backup API is documented. |
| Import and Export | Import SQL files and export databases as full, schema-only, or data-only | pattern | medium | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | Medium | Possible via custom admin actions or request handlers, but not a built-in product surface. |
| Indexes and Performance | Create, drop, and optimize indexes for query performance | native | medium | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Medium | Standard SQLite indexing is available, but high-scale performance characteristics must be re-benchmarked on per-actor storage. |
| SQL Statements and SQLite Compatibility | Supported SQL syntax, PRAGMA statements, and SQLite extensions | partial | medium | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Medium | Core SQLite works. Extension/PRAGMA parity is not fully documented, so validate anything beyond mainstream SQLite syntax. |
| JSON Querying | Built-in JSON functions for extracting, modifying, and querying JSON data | partial | low | [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | Medium | Likely available through SQLite itself, but there is no Rivet-specific validation coverage yet. |
| Foreign Keys | Referential integrity constraints with CASCADE, RESTRICT, and SET NULL actions | partial | low | [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | Medium | Expected from SQLite, but not explicitly covered by repo tests today. |
| Generated Columns | Virtual and stored columns derived from expressions or JSON extraction | partial | low | [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | Medium | Treat as a migration spike item until validated on Rivet actor-local SQLite. |
| Read Replication | Global read replicas with Sessions API for reduced latency | unsupported | high | [Metadata](https://rivet.dev/docs/actors/metadata) | Gap | High | Rivet has no D1 Sessions or bookmark-consistent read-replica model. |
| REST API | HTTP endpoints for database management, queries, import/export, and time travel | pattern | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | Medium | Build actor-scoped DB APIs with `onRequest`; management surface is application-defined. |
| Environments | Staging and production environment configurations with separate databases | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers/README.md](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/README.md) | Low | Use namespaces/env vars or separate deployments. |
| Data Location and Jurisdiction | Jurisdiction constraints and location hints for regulatory compliance | partial | medium | [Metadata](https://rivet.dev/docs/actors/metadata), [Multi-Region](https://rivet.dev/docs/self-hosting/multi-region) | Docs-only | High | Rivet exposes region awareness but not D1-style jurisdiction pinning in the actor API. |
| Local Development | Fully-featured local D1 via Wrangler with persistent state | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers), [Testing](https://rivet.dev/docs/actors/testing) | [examples/cloudflare-workers](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/cloudflare-workers), [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Low | Local development for actor-local SQLite is well-covered. |
| Remote Development | Browser-based development via Cloudflare dashboard playground | out-of-scope | high | [Debugging](https://rivet.dev/docs/actors/debugging) | Gap | Low | Rivet does not expose a D1-style hosted playground. |
| Retry Logic | Automatic read query retries and recommended write retry patterns | pattern | medium | [Workflows](https://rivet.dev/docs/actors/workflows), [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) | Medium | Retries are app-defined in workflows/queues rather than automatic at the SQL client layer. |
| Observability and Metrics | Seven key metrics with 31-day retention via dashboard and GraphQL API | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Medium | Inspector and logs exist, but no D1-equivalent managed metrics layer is documented. |
| Billing and Usage | Usage tracking for rows read, rows written, and storage | out-of-scope | high | [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | Low | Rivet does not expose D1-style row metering. |
| Audit Logs | Account-level change tracking for database operations | out-of-scope | high | [Debugging](https://rivet.dev/docs/actors/debugging) | Gap | Medium | No actor-local audit-log surface is documented. |
| Debugging and Error Handling | Error prefixes, retryable scenarios, and debugging tools | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging), [Errors](https://rivet.dev/docs/actors/errors) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Low | Rich debugging exists, but not the same D1-specific error taxonomy. |
| Data Security | AES-256 encryption at rest and TLS encryption in transit | partial | medium | [Authentication](https://rivet.dev/docs/actors/authentication) | Docs-only | Medium | App/auth surfaces are documented, but storage-at-rest claims for actor-local SQLite are not expressed the same way as D1. |
| Pricing | Row-based and storage-based pricing with free and paid tiers | out-of-scope | high | [Actors Index](https://rivet.dev/docs/actors) | Docs-only | Low | Pricing is a platform/commercial comparison, not a runtime feature. |
| Limits | Database size, query, throughput, and account-level constraints | partial | high | [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | High | Rivet actor-local storage limits are materially different from D1 limits. |
| Full-Text Search (FTS5) | FTS5 module for efficient text searching across datasets | partial | low | [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | Medium | Likely depends on underlying SQLite build; validate before committing to migration. |
| Type Conversion | Automatic JavaScript to SQLite type conversion rules | partial | medium | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Low | Conversion exists, but result envelopes and nullability behavior should be compared case-by-case. |
| Wrangler Commands Reference | CLI commands for database management, migrations, and time travel | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Gap | Low | Rivet has no D1-equivalent admin CLI surface. |
| Horizontal Scaling Model | Scale-out design across multiple smaller databases with sharding | pattern | high | [Design Patterns](https://rivet.dev/docs/actors/design-patterns), [Scaling](https://rivet.dev/docs/actors/scaling) | Docs-only | Medium | Actor sharding is a strong fit, but shard keys and routing become app responsibilities. |

## High-Risk Behavioral Deltas

- **D1 is shared; Rivet SQLite is actor-local.** This is the most important migration gap. If the D1 database is shared across many logical entities, you must decide whether to collapse it into one actor, shard it across many actors, or move that workload to external PostgreSQL.
- **Cross-entity transactions do not become magically distributed.** SQLite transactions are native inside one actor, but there is no D1 Sessions/bookmark model and no cross-actor ACID story.
- **Managed durability tooling is missing.** PITR, import/export, and backup flows are not documented for actor-local SQLite today.
- **Read-replica consistency semantics do not map.** If the Cloudflare design relies on D1 session bookmarks or replica reads, keep that part external or redesign around actor ownership.
- **Capacity planning must be redone.** D1 limits and billing units do not map to Rivet Actor limits or cost structure.

## Validation Checklist

| Test case | Expected result | Pass/fail evidence link |
|-----------|-----------------|-------------------------|
| Actor-local SQLite bootstraps schema | Migration/init runs on actor startup | Pass: [examples/sqlite-drizzle](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/sqlite-drizzle) |
| CRUD and transactions work | Inserts, updates, deletes, and rollback semantics hold | Pass: [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) |
| SQLite persists through sleep/wake | Data remains after actor sleep cycles | Pass: [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) |
| Shared-DB topology is explicit | Migration doc says whether each D1 DB maps to one actor, many actors, or external Postgres | Gap: review [Scaling](https://rivet.dev/docs/actors/scaling) and [Design Patterns](https://rivet.dev/docs/actors/design-patterns) in the migration design |
| Foreign keys and generated columns behave as expected | Schema-level SQLite features work on the target actor | Gap: only generic [SQLite docs](https://rivet.dev/docs/actors/sqlite) exist today; add a migration spike proof |
| FTS and JSON functions are proven | Any advanced SQLite extension usage is tested on Rivet | Gap: only generic [SQLite docs](https://rivet.dev/docs/actors/sqlite) exist today; add a migration spike proof |
| PITR/backups replacement exists | Ops runbook covers recovery and export paths | Fail: [SQLite docs](https://rivet.dev/docs/actors/sqlite) do not document built-in PITR/backup/export features |

---

> Cloudflare D1 is a managed, serverless database with SQLite's SQL semantics, built-in disaster recovery, and Worker and HTTP API access. It is designed for horizontal scale-out across multiple smaller (10 GB) databases.

---

## Database Creation and Management

**Docs:** https://developers.cloudflare.com/d1/get-started/

D1 databases are created via the Wrangler CLI or REST API. Each database gets a unique UUID and is bound to Workers through configuration. Databases can be created with optional location hints and jurisdiction constraints.

**Create a database via Wrangler:**
```bash
npx wrangler@latest d1 create prod-d1-tutorial
```

**List databases:**
```bash
npx wrangler d1 list
```

**Get database info:**
```bash
npx wrangler d1 info <DATABASE_NAME> --json
```

**Delete a database:**
```bash
npx wrangler d1 delete <DATABASE_NAME>
```

**Create a database via REST API:**
```bash
curl -X POST "https://api.cloudflare.com/client/v4/accounts/<account_id>/d1/database" \
     -H "Authorization: Bearer $TOKEN" \
     -H "Content-Type: application/json" \
     --data '{"name": "my-database"}'
```

**Limits:**
- 50,000 databases per account (Workers Paid) / 10 (Free)
- Maximum database size: 10 GB (Workers Paid) / 500 MB (Free)
- Maximum storage per account: 1 TB (Workers Paid) / 5 GB (Free)

---

## Bindings and Configuration

**Docs:** https://developers.cloudflare.com/d1/get-started/

D1 databases are connected to Workers via bindings declared in the Wrangler configuration file. The binding name becomes the variable used to access the database in code.

**wrangler.jsonc:**
```json
{
  "d1_databases": [
    {
      "binding": "prod_d1_tutorial",
      "database_name": "prod-d1-tutorial",
      "database_id": "<unique-ID-for-your-database>"
    }
  ]
}
```

**wrangler.toml:**
```toml
[[d1_databases]]
binding = "prod_d1_tutorial"
database_name = "prod-d1-tutorial"
database_id = "<unique-ID-for-your-database>"
```

The binding is accessed in the Worker via `env.<BINDING_NAME>` (JavaScript) or `self.env.<BINDING_NAME>` (Python).

---

## Worker Binding API: D1Database Methods

**Docs:** https://developers.cloudflare.com/d1/worker-api/d1-database/

The D1Database object is accessed through the Worker environment binding and provides the following methods:

### prepare()

Prepares a query statement for later execution. Use with `bind()` to safely insert parameterized values (prevents SQL injection).

```js
const someVariable = `Bs Beverages`;
const stmt = env.DB.prepare("SELECT * FROM Customers WHERE CompanyName = ?").bind(someVariable);
```

### batch()

Sends multiple SQL statements in one call. Statements execute sequentially and non-concurrently. If any statement fails, the entire batch rolls back.

```js
const companyName1 = `Bs Beverages`;
const companyName2 = `Around the Horn`;
const stmt = env.DB.prepare(`SELECT * FROM Customers WHERE CompanyName = ?`);
const batchResult = await env.DB.batch([
  stmt.bind(companyName1),
  stmt.bind(companyName2)
]);
```

### exec()

Executes SQL without prepared statements or bindings. Returns a `D1ExecResult` with `count` and `duration`. Should only be used for maintenance tasks due to poorer performance.

```js
const returnValue = await env.DB.exec(`SELECT * FROM Customers WHERE CompanyName = "Bs Beverages"`);
```

### dump()

Exports the entire D1 database as an SQLite-compatible ArrayBuffer. Only available for alpha-period databases.

```js
const dump = await db.dump();
return new Response(dump, {
  status: 200,
  headers: {
    "Content-Type": "application/octet-stream",
  },
});
```

### withSession()

Creates a `D1DatabaseSession` that maintains sequential consistency across queries. Used for read replication. Accepts parameters: `"first-primary"`, `"first-unconstrained"` (default), or a bookmark string.

```js
const session = env.DB.withSession("<parameter>");
```

### getBookmark()

Retrieves the latest bookmark from a D1 Session, returning a string identifying the database version or null if no queries have been executed.

```js
const session = env.DB.withSession("first-primary");
const result = await session
  .prepare(`SELECT * FROM Customers WHERE CompanyName = 'Bs Beverages'`)
  .run()
const { bookmark } = session.getBookmark();
return bookmark;
```

---

## Prepared Statement Methods

**Docs:** https://developers.cloudflare.com/d1/worker-api/prepared-statements/

### bind()

Attaches parameters to a prepared statement. Supports both ordered (`?NNN`) and anonymous (`?`) parameter formats following SQLite conventions.

**Anonymous parameters:**
```js
const stmt = db.prepare("SELECT * FROM Customers WHERE CompanyName = ?").bind("");
```

**Multiple anonymous parameters:**
```js
const stmt = db.prepare("SELECT * FROM Customers WHERE CompanyName = ? AND CustomerId = ?").bind("Alfreds Futterkiste", 1);
```

**Ordered parameters:**
```js
const stmt = db.prepare("SELECT * FROM Customers WHERE CompanyName = ?2 AND CustomerId = ?1").bind(1, "Alfreds Futterkiste");
```

### run()

Executes the prepared query and returns a `D1Result` object containing success status, metadata, and results array. Results is empty for write operations (UPDATE, DELETE, INSERT). Functionally equivalent to `all()`.

```js
const someVariable = `Bs Beverages`;
const stmt = env.DB.prepare("SELECT * FROM Customers WHERE CompanyName = ?").bind(someVariable);
const returnValue = await stmt.run();
return Response.json(returnValue);
```

**Response:**
```json
{
  "success": true,
  "meta": {
    "served_by": "miniflare.db",
    "duration": 1,
    "changes": 0,
    "last_row_id": 0,
    "changed_db": false,
    "size_after": 8192,
    "rows_read": 4,
    "rows_written": 0
  },
  "results": [
    {
      "CustomerId": 11,
      "CompanyName": "Bs Beverages",
      "ContactName": "Victoria Ashworth"
    },
    {
      "CustomerId": 13,
      "CompanyName": "Bs Beverages",
      "ContactName": "Random Name"
    }
  ]
}
```

### raw()

Executes the query and returns results as an array of arrays (no metadata). Optionally accepts `{ columnNames: true }` to include column names as the first row.

```js
const returnValue = await stmt.raw();
```

**Response:**
```json
[
  [11, "Bs Beverages", "Victoria Ashworth"],
  [13, "Bs Beverages", "Random Name"]
]
```

**With column names:**
```js
const returnValue = await stmt.raw({columnNames:true});
```

**Response:**
```json
[
  ["CustomerId", "CompanyName", "ContactName"],
  [11, "Bs Beverages", "Victoria Ashworth"],
  [13, "Bs Beverages", "Random Name"]
]
```

### first()

Returns only the first row as an object. Accepts an optional `columnName` parameter to extract a specific column value. Returns `null` if no results exist. Does not alter the SQL query -- to improve performance, append `LIMIT 1` to your statement.

```js
const someVariable = `Bs Beverages`;
const stmt = env.DB.prepare("SELECT * FROM Customers WHERE CompanyName = ?").bind(someVariable);
const returnValue = await stmt.first();
return Response.json(returnValue);
```

**Response (all columns):**
```json
{
  "CustomerId": 11,
  "CompanyName": "Bs Beverages",
  "ContactName": "Victoria Ashworth"
}
```

**Specific column:**
```js
const returnValue = await stmt.first("CustomerId");
```

**Response:**
```json
11
```

---

## Return Objects

**Docs:** https://developers.cloudflare.com/d1/worker-api/return-object/

### D1Result

Returned by `D1PreparedStatement::run()` and `D1Database::batch()`. Contains:

- `success` (boolean) - whether the operation succeeded
- `meta` object with:
  - `served_by` - the instance that served the request
  - `served_by_region` - the region that served the request
  - `served_by_primary` - whether the primary instance served it
  - `timings` - timing information
  - `duration` - operation duration in milliseconds
  - `changes` - number of rows changed
  - `last_row_id` - last inserted row ID
  - `changed_db` - whether the database was changed
  - `size_after` - database size after operation
  - `rows_read` - number of rows read/scanned
  - `rows_written` - number of rows written
  - `total_attempts` - total query attempts
- `results` - array of result rows (or null for write operations)

### D1ExecResult

Returned by `D1Database::exec()`. Contains:

- `count` - the number of executed queries
- `duration` - the duration of the operation, in milliseconds

**Important:** Any numeric value in a column is affected by JavaScript's 52-bit precision for numbers. Very large int64 values may lose precision when retrieved.

---

## SQL Query Execution via Wrangler

**Docs:** https://developers.cloudflare.com/d1/wrangler-commands/

Execute SQL commands or files directly against a database via the CLI.

**Execute a command:**
```bash
npx wrangler d1 execute prod-d1-tutorial --local --command="SELECT * FROM Customers"
```

**Execute a SQL file:**
```bash
npx wrangler d1 execute prod-d1-tutorial --remote --file=./schema.sql
```

**Schema example (schema.sql):**
```sql
DROP TABLE IF EXISTS Customers;
CREATE TABLE IF NOT EXISTS Customers (CustomerId INTEGER PRIMARY KEY, CompanyName TEXT, ContactName TEXT);
INSERT INTO Customers (CustomerID, CompanyName, ContactName) VALUES (1, 'Alfreds Futterkiste', 'Maria Anders'), (4, 'Around the Horn', 'Thomas Hardy'), (11, 'Bs Beverages', 'Victoria Ashworth'), (13, 'Bs Beverages', 'Random Name');
```

Supports `--local`, `--remote`, and `--preview` flags for targeting different execution contexts.

---

## Complete Worker Example

**Docs:** https://developers.cloudflare.com/d1/get-started/

**JavaScript:**
```js
export default {
  async fetch(request, env) {
    const { pathname } = new URL(request.url);
    if (pathname === "/api/beverages") {
      const { results } = await env.prod_d1_tutorial
        .prepare("SELECT * FROM Customers WHERE CompanyName = ?")
        .bind("Bs Beverages")
        .run();
      return Response.json(results);
    }
    return new Response(
      "Call /api/beverages to see everyone who works at Bs Beverages",
    );
  },
};
```

**TypeScript:**
```typescript
export interface Env {
  prod_d1_tutorial: D1Database;
}
export default {
  async fetch(request, env): Promise<Response> {
    const { pathname } = new URL(request.url);
    if (pathname === "/api/beverages") {
      const { results } = await env.prod_d1_tutorial.prepare(
        "SELECT * FROM Customers WHERE CompanyName = ?",
      )
        .bind("Bs Beverages")
        .run();
      return Response.json(results);
    }
    return new Response(
      "Call /api/beverages to see everyone who works at Bs Beverages",
    );
  },
} satisfies ExportedHandler<Env>;
```

---

## Migrations

**Docs:** https://developers.cloudflare.com/d1/reference/migrations/

Database migrations are a way of versioning your database. Each migration is stored as a `.sql` file in a `migrations` folder. The system provides create, list, and apply capabilities.

**Create a migration:**
```bash
npx wrangler d1 migrations create <DATABASE_NAME> <DESCRIPTION>
```
This generates a file like `0000_description.sql`.

**List unapplied migrations:**
```bash
npx wrangler d1 migrations list <DATABASE_NAME> --local
npx wrangler d1 migrations list <DATABASE_NAME> --remote
```

**Apply migrations:**
```bash
npx wrangler d1 migrations apply <DATABASE_NAME> --local
npx wrangler d1 migrations apply <DATABASE_NAME> --remote
```

Applied migrations are tracked in a `d1_migrations` table. Both the migrations directory and tracking table name are customizable in Wrangler configuration via `migrations_dir` and `migrations_table` parameters.

When migrations would violate foreign key constraints, call `PRAGMA defer_foreign_keys = true` beforehand to temporarily disable them during schema changes.

---

## Time Travel (Point-in-Time Recovery)

**Docs:** https://developers.cloudflare.com/d1/reference/time-travel/

Time Travel is D1's approach to backups and point-in-time recovery. It enables restoring a database to any minute within the last 30 days (Workers Paid) or 7 days (Free). It requires no manual enablement and operates automatically at no additional cost.

The system generates bookmarks representing database states at specific moments. Bookmarks older than 30 days are invalid and cannot be used as a restore point. Restoring to an earlier bookmark preserves access to newer ones.

**Retrieve bookmark info:**
```bash
wrangler d1 time-travel info YOUR_DATABASE
```

**Restore to a timestamp:**
```bash
wrangler d1 time-travel restore YOUR_DATABASE --timestamp=UNIX_TIMESTAMP
```

**Restore to a bookmark:**
```bash
wrangler d1 time-travel restore YOUR_DATABASE --bookmark=BOOKMARK_ID
```

Supported timestamp formats:
- Unix timestamps (seconds since January 1, 1970 UTC)
- ISO-8601 date-time strings (e.g., `2023-07-27T11:18:53.000-04:00`)

**Important:** Restoration is destructive, overwriting existing database contents in place. You can undo a restore by noting the previous bookmark from the operation output.

**Limits:**
- Maximum 10 restore operations per 10 minutes per database

---

## Legacy Backups

**Docs:** https://developers.cloudflare.com/d1/reference/backups/

Legacy snapshot-based backups for alpha databases. Access will be removed on 2025-07-01. Production databases should use Time Travel instead.

**List backups:**
```bash
wrangler d1 backup list existing-db
```

**Create a manual backup:**
```bash
wrangler d1 backup create example-db
```

**Download a backup:**
```bash
wrangler d1 backup download example-db <BACKUP_ID>
```

**Restore from a backup:**
```bash
wrangler d1 backup restore existing-db <BACKUP_ID>
```

Automatic backups are created hourly and retained for 24 hours. Restoring a backup overwrites the existing database in place.

---

## Import and Export

**Docs:** https://developers.cloudflare.com/d1/best-practices/import-export-data/

### Importing Data

Import existing SQLite tables and data directly into D1 using the `wrangler d1 execute` command with the `--file` flag. Requires SQL format (not raw `.sqlite3` files).

```bash
npx wrangler d1 execute example-db --remote --file=users_export.sql
```

**Converting SQLite files to SQL:**
```bash
sqlite3 db_dump.sqlite3 .dump > db.sql
```

Then edit the output to remove `BEGIN TRANSACTION`, `COMMIT;`, and any `_cf_KV` table creation statements.

**Limit:** File imports are limited to 5 GB.

**Foreign key handling during import:**
```sql
PRAGMA defer_foreign_keys = true
```

### Exporting Data

**Full database export:**
```bash
npx wrangler d1 export <database_name> --remote --output=./database.sql
```

**Schema-only export:**
```bash
npx wrangler d1 export <database_name> --remote --output=./schema.sql --no-data
```

**Data-only export:**
```bash
npx wrangler d1 export <database_name> --remote --output=./data.sql --no-schema
```

**Single table export:**
```bash
npx wrangler d1 export <database_name> --remote --output=./table.sql --table=<table_name>
```

Limitations: Virtual tables (including FTS5) cannot be exported. Running exports block other database requests.

---

## Indexes and Performance

**Docs:** https://developers.cloudflare.com/d1/best-practices/use-indexes/

Indexes improve query performance by reducing the number of rows scanned. They also enforce uniqueness constraints.

**Create an index:**
```sql
CREATE INDEX IF NOT EXISTS idx_orders_customer_id ON orders(customer_id)
```

**Create a unique index:**
```sql
CREATE UNIQUE INDEX idx_users_email ON users(email_address)
```

**List all indexes:**
```sql
SELECT name, type, sql FROM sqlite_schema WHERE type IN ('index');
```

**Test index usage with EXPLAIN QUERY PLAN:**
```sql
EXPLAIN QUERY PLAN SELECT * FROM users WHERE email_address = 'foo@example.com';
-- QUERY PLAN
-- `--SEARCH users USING INDEX idx_email_address (email_address=?)
```

**Multi-column indexes:**
```sql
CREATE INDEX idx_customer_id_transaction_date ON transactions(customer_id, transaction_date)
```

Multi-column index usage rules: queries use the index only if they specify all columns, or a subset where all columns to the "left" are included. For example, with `(customer_id, transaction_date)`:
- `WHERE customer_id = '1234' AND transaction_date = '2023-03-25'` -- uses index
- `WHERE transaction_date = '2023-03-28'` -- does NOT use index
- `WHERE customer_id = '56789'` -- uses index

**Partial indexes:**
```sql
CREATE INDEX idx_order_status_not_complete ON orders(order_status) WHERE order_status != 6
```

Partial indexes are faster at read time (fewer rows) and write time (fewer index updates).

**Drop an index:**
```sql
DROP INDEX index_name
```

**After creating an index, run PRAGMA optimize:**
```sql
PRAGMA optimize
```

Key considerations:
- Indexes are not always a free performance boost; create only on frequently queried columns
- Indexes cannot be modified, only dropped and recreated
- Indexes contribute to overall storage (an index is effectively a table itself)
- Indexes cannot reference other tables or non-deterministic functions

---

## SQL Statements and SQLite Compatibility

**Docs:** https://developers.cloudflare.com/d1/sql-api/sql-statements/

D1 uses SQLite's query engine and supports most SQLite SQL conventions. Supported extensions include:
- **FTS5** module for full-text search
- **JSON extension** for JSON functions and operators
- **Math functions**

### PRAGMA Statements

Key PRAGMA commands supported:

| PRAGMA | Description |
|--------|-------------|
| `PRAGMA table_list` | Returns all tables and views with schema, type, column count, strictness |
| `PRAGMA table_info("TABLE_NAME")` | Shows columns: cid, name, type, notnull, dflt_value, pk |
| `PRAGMA table_xinfo("TABLE_NAME")` | Like table_info but includes generated columns |
| `PRAGMA index_list("TABLE_NAME")` | Displays indexes with sequence, name, uniqueness, origin, partial |
| `PRAGMA index_info(INDEX_NAME)` | Shows indexed columns |
| `PRAGMA quick_check` | Checks table formatting and consistency |
| `PRAGMA foreign_key_check` | Validates foreign key references |
| `PRAGMA foreign_key_list("TABLE_NAME")` | Lists foreign key constraints |
| `PRAGMA case_sensitive_like = (on\|off)` | Controls LIKE case sensitivity (default: off) |
| `PRAGMA ignore_check_constraints = (on\|off)` | Toggles CHECK constraint enforcement |
| `PRAGMA legacy_alter_table = (on\|off)` | Controls ALTER TABLE RENAME behavior |
| `PRAGMA recursive_triggers = (on\|off)` | Allows triggers to activate other triggers |
| `PRAGMA foreign_keys = (on\|off)` | Enforces foreign key constraints (default: off) |
| `PRAGMA defer_foreign_keys = (on\|off)` | Defers constraint checks until transaction end |
| `PRAGMA optimize` | Runs ANALYZE to update statistics for query planning |

---

## JSON Querying

**Docs:** https://developers.cloudflare.com/d1/sql-api/query-json/

D1 supports built-in JSON querying and parsing via SQLite's JSON extension. JSON data is stored as `TEXT` columns.

### Supported JSON Functions

| Function | Purpose |
|----------|---------|
| `json(json)` | Validates and minifies JSON |
| `json_array(value1, value2...)` | Creates JSON arrays |
| `json_array_length(json, path)` | Returns array length |
| `json_extract(json, path)` | Extracts values at specified paths |
| `json -> path` | Returns extracted value as JSON |
| `json ->> path` | Returns extracted value as SQL type |
| `json_insert(json, path, value)` | Inserts without overwriting |
| `json_object(label1, value1...)` | Creates JSON objects |
| `json_patch(target, patch)` | Merges using MergePatch |
| `json_remove(json, path...)` | Removes keys at paths |
| `json_replace(json, path, value)` | Overwrites existing values |
| `json_set(json, path, value)` | Inserts or overwrites |
| `json_type(json, path)` | Returns value type |
| `json_valid(json)` | Validates JSON (0 or 1) |
| `json_quote(value)` | Converts SQL values to JSON |
| `json_group_array(value)` | Returns values as JSON array |
| `json_each(value, path)` | Expands top-level elements as rows |
| `json_tree(value, path)` | Expands full object as rows |

**Path syntax:** `$` = top-level, `$.key1.key2` = nested, `$.key[2]` = array index.

**Extract temperature from JSON:**
```sql
SELECT json_extract(sensor_reading, '$.measurement.temp_f') FROM readings;
```

**Insert into a JSON array:**
```sql
UPDATE users
SET login_history = json_insert(login_history, '$.history[#]', '2023-05-15T20:33:06+00:00')
WHERE user_id = 'aba0e360-1e04-41b3-91a0-1f2263e1e0fb'
```

**Use json_each for IN queries:**
```sql
UPDATE users
SET last_audited = '2023-05-16T11:24:08+00:00'
WHERE id IN (SELECT value FROM json_each('[183183, 13913, 94944]'))
```

**TypeScript implementation with json_each:**
```ts
const stmt = context.env.DB
    .prepare("UPDATE users SET last_audited = ? WHERE id IN (SELECT value FROM json_each(?1))")
const resp = await stmt.bind(
    "2023-05-16T11:24:08+00:00",
    JSON.stringify([183183, 13913, 94944])
    ).run()
```

---

## Foreign Keys

**Docs:** https://developers.cloudflare.com/d1/sql-api/foreign-keys/

D1 enforces foreign key constraints within all queries and migrations (equivalent to `PRAGMA foreign_keys = on`).

**Creating foreign keys:**
```sql
CREATE TABLE users (
    user_id INTEGER PRIMARY KEY,
    email_address TEXT,
    name TEXT,
    metadata TEXT
)

CREATE TABLE orders (
    order_id INTEGER PRIMARY KEY,
    status INTEGER,
    item_desc TEXT,
    shipped_date INTEGER,
    user_who_ordered INTEGER,
    FOREIGN KEY(user_who_ordered) REFERENCES users(user_id)
)
```

**Available foreign key actions:**
- `CASCADE` - child rows are deleted/updated when parent changes
- `RESTRICT` - parent cannot change if children reference it
- `SET DEFAULT` - child columns adopt their schema default
- `SET NULL` - child columns become NULL
- `NO ACTION` - no automatic changes occur

**Deferring constraints during migrations:**
```sql
PRAGMA defer_foreign_keys = on
```

---

## Generated Columns

**Docs:** https://developers.cloudflare.com/d1/reference/generated-columns/

Generated columns are derived from other columns, SQL functions, or extracted JSON values. Two types:
- **VIRTUAL** (default): generated on read, no storage cost, more compute at read time
- **STORED**: generated on write, consumes storage, faster reads

**Create table with generated column:**
```sql
CREATE TABLE sensor_readings (
    event_id INTEGER PRIMARY KEY,
    timestamp INTEGER NOT NULL,
    raw_data TEXT,
    location as (json_extract(raw_data, '$.measurement.location')) STORED
);
```

**Add generated column to existing table:**
```sql
ALTER TABLE sensor_readings
ADD COLUMN location as (json_extract(raw_data, '$.measurement.location'));
```

**Date formatting from Unix timestamp:**
```sql
ALTER TABLE your_table
ADD COLUMN formatted_date AS (date(timestamp, 'unixepoch'))
```

**Expiration date calculation:**
```sql
ALTER TABLE your_table
ADD COLUMN expires_at AS (date(timestamp, '+30 days'));
```

Constraints:
- Tables require at least one non-generated column
- Expressions must reference only columns in the same row
- Must use deterministic functions only (`random()`, sub-queries, aggregations are prohibited)
- Columns added via `ALTER TABLE` must be `VIRTUAL`

---

## Read Replication

**Docs:** https://developers.cloudflare.com/d1/best-practices/read-replication/

Read replication reduces latency by creating read-only database copies (replicas) across regions globally. The Sessions API is mandatory for read replication. Without it, all queries go to the primary database only.

Replicas automatically deploy to: ENAM, WNAM, WEUR, EEUR, APAC, and OC.

**Enable read replication via REST API:**
```bash
curl -X PUT "https://api.cloudflare.com/client/v4/accounts/{account_id}/d1/database/{database_id}" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"read_replication": {"mode": "auto"}}'
```

**Session without constraints (routes to nearest replica):**
```typescript
const session = env.DB.withSession()
const result = await session
  .prepare(`SELECT * FROM Customers WHERE CompanyName = 'Bs Beverages'`)
  .run()
```

**Session with primary-first constraint (first query goes to primary):**
```typescript
const session = env.DB.withSession(`first-primary`)
const result = await session
  .prepare(`SELECT * FROM Customers WHERE CompanyName = 'Bs Beverages'`)
  .run()
```

**Session from previous bookmark (ensures consistency across requests):**
```typescript
const bookmark = request.headers.get('x-d1-bookmark') ?? 'first-unconstrained';
const session = env.DB.withSession(bookmark)
const result = await session
  .prepare(`SELECT * FROM Customers WHERE CompanyName = 'Bs Beverages'`)
  .run()
response.headers.set('x-d1-bookmark', session.getBookmark() ?? "")
```

**Full handler example with sessions:**
```javascript
export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const bookmark = request.headers.get("x-d1-bookmark") ?? "first-unconstrained";
    const session = env.DB01.withSession(bookmark);
    try {
      const response = await withTablesInitialized(request, session, handleRequest);
      response.headers.set("x-d1-bookmark", session.getBookmark() ?? "");
      return response;
    } catch (e) {
      console.error({
        message: "Failed to handle request",
        error: String(e),
        errorProps: e,
        url,
        bookmark,
      });
      return Response.json(
        { error: String(e), errorDetails: e },
        { status: 500 },
      );
    }
  },
};
```

**Check which instance served a request:**
```typescript
const result = await env.DB.withSession()
  .prepare(`SELECT * FROM Customers WHERE CompanyName = 'Bs Beverages'`)
  .run();
console.log({
  servedByRegion: result.meta.served_by_region ?? "",
  servedByPrimary: result.meta.served_by_primary ?? "",
});
```

Read replication is built into D1 -- no extra storage or compute costs for replicas. The Sessions API ensures sequential consistency for all queries in a session.

---

## REST API

**Docs:** https://developers.cloudflare.com/api/resources/d1/

The D1 REST API provides HTTP access for database management and querying. Best suited for administrative use (subject to global Cloudflare API rate limits).

### Database Management Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/accounts/{account_id}/d1/database` | List D1 databases |
| `GET` | `/accounts/{account_id}/d1/database/{database_id}` | Get a specific D1 database |
| `POST` | `/accounts/{account_id}/d1/database` | Create a D1 database |
| `PUT` | `/accounts/{account_id}/d1/database/{database_id}` | Update a D1 database |
| `PATCH` | `/accounts/{account_id}/d1/database/{database_id}` | Partially update a D1 database |
| `DELETE` | `/accounts/{account_id}/d1/database/{database_id}` | Delete a D1 database |

### Query Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/accounts/{account_id}/d1/database/{database_id}/query` | Query D1 database (returns results as objects) |
| `POST` | `/accounts/{account_id}/d1/database/{database_id}/raw` | Query D1 database (returns results as arrays) |

The `/query` endpoint accepts a `sql` string that can contain multiple SQLite statements and responds with an array of results, one for each statement.

**Example REST API query:**
```bash
curl -X POST "https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/d1/database/{db_id}/raw" \
  -H "Authorization: Bearer {CLOUDFLARE_TOKEN}" \
  -H "Content-Type: application/json" \
  --data '{"sql": "CREATE TABLE IF NOT EXISTS marvel (name TEXT, power INTEGER); SELECT name, type FROM sqlite_master ORDER BY name ASC;"}'
```

### Import/Export Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/accounts/{account_id}/d1/database/{database_id}/export` | Export D1 database as SQL |
| `POST` | `/accounts/{account_id}/d1/database/{database_id}/import` | Import SQL into D1 database |

### Time Travel Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/accounts/{account_id}/d1/database/{database_id}/time_travel/bookmark` | Get current bookmark or nearest bookmark at/before a timestamp |
| `POST` | `/accounts/{account_id}/d1/database/{database_id}/time_travel/restore` | Restore to a bookmark or point in time |

---

## Environments

**Docs:** https://developers.cloudflare.com/d1/configuration/environments/

Environments enable deploying identical projects with different configurations (e.g., staging vs. production databases).

**wrangler.jsonc:**
```jsonc
{
  "env": {
    "staging": {
      "d1_databases": [
        {
          "binding": "<BINDING_NAME_1>",
          "database_name": "<DATABASE_NAME_1>",
          "database_id": "<UUID1>"
        }
      ]
    },
    "production": {
      "d1_databases": [
        {
          "binding": "<BINDING_NAME_2>",
          "database_name": "<DATABASE_NAME_2>",
          "database_id": "<UUID2>"
        }
      ]
    }
  }
}
```

**wrangler.toml:**
```toml
[[env.staging.d1_databases]]
binding = "<BINDING_NAME_1>"
database_name = "<DATABASE_NAME_1>"
database_id = "<UUID1>"

[[env.production.d1_databases]]
binding = "<BINDING_NAME_2>"
database_name = "<DATABASE_NAME_2>"
database_id = "<UUID2>"
```

---

## Data Location and Jurisdiction

**Docs:** https://developers.cloudflare.com/d1/configuration/data-location/

By default, D1 creates your primary database instance close to where the creation request originated. You can control location via jurisdiction constraints and location hints.

### Jurisdictions

Jurisdictions constrain databases to specific regions for regulatory compliance (GDPR, FedRAMP). Must be set at creation time and cannot be changed afterward.

| Jurisdiction | Description |
|--------------|-------------|
| `eu` | European Union |
| `fedramp` | FedRAMP-compliant data centers |

**Create with jurisdiction via Wrangler:**
```bash
npx wrangler@latest d1 create db-with-jurisdiction --jurisdiction=eu
```

**Create with jurisdiction via REST API:**
```bash
curl -X POST "https://api.cloudflare.com/client/v4/accounts/<account_id>/d1/database" \
     -H "Authorization: Bearer $TOKEN" \
     -H "Content-Type: application/json" \
     --data '{"name": "db-with-jurisdiction", "jurisdiction": "eu"}'
```

### Location Hints

Location hints are optional geographical preferences for the primary instance. They do not guarantee placement but route to the nearest possible location.

| Hint | Region |
|------|--------|
| `wnam` | Western North America |
| `enam` | Eastern North America |
| `weur` | Western Europe |
| `eeur` | Eastern Europe |
| `apac` | Asia-Pacific |
| `oc` | Oceania |

South America, Africa, and the Middle East are not currently supported.

When read replication is enabled, replicas deploy to all available regions. With a jurisdiction constraint, replicas remain within that jurisdiction only.

---

## Local Development

**Docs:** https://developers.cloudflare.com/d1/best-practices/local-development/

D1 has fully-featured support for local development, running the same version of D1 as Cloudflare runs globally. Uses Wrangler to manage local sessions and state.

**Start local development:**
```bash
npx wrangler dev
```

Key behaviors:
- Local and production (remote) data are separated
- Data persists across `wrangler dev` runs in Wrangler v3+
- Custom persistence path: `wrangler dev --persist-to=/path/to/file`
- Remote database access requires `"remote": true` in binding configuration

**Pages local development requires `preview_database_id`:**
```bash
wrangler d1 execute YOUR_DATABASE_NAME --local --command "SELECT * FROM my_table"
```

### Programmatic Testing

**Miniflare:** Simulates Workers and D1 using production runtime code. Use `getD1Database()` to retrieve a simulated database.

**`unstable_dev()` API:** Launches a local HTTP server for testing with D1 bindings.

---

## Remote Development

**Docs:** https://developers.cloudflare.com/d1/best-practices/remote-development/

D1 supports remote development via the Cloudflare dashboard playground, a browser-based Visual Studio Code environment.

**Verification code:**
```js
export default {
  async fetch(request, env, ctx) {
    const res = await env.DB.prepare("SELECT 1;").run();
    return new Response(JSON.stringify(res, null, 2));
  },
};
```

---

## Retry Logic

**Docs:** https://developers.cloudflare.com/d1/best-practices/retry-queries/

D1 automatically retries read-only queries up to two more times when it encounters a retryable error. For write queries, applications should implement retry logic with exponential backoff and jitter.

Retryable error messages include:
- "Network connection lost"
- "storage caused object to be reset"
- "reset because its code was updated"

The recommended approach uses a `shouldRetry()` function with a maximum of 5 retry attempts when retryable errors occur.

**TypeScript retry implementation:**
```typescript
import { tryWhile } from "@cloudflare/actors";

function queryD1Example(d1: D1Database, sql: string) {
  return await tryWhile(async () => {
    return await d1.prepare(sql).run();
  }, shouldRetry);
}

function shouldRetry(err: unknown, nextAttempt: number) {
  const errMsg = String(err);
  const isRetryableError =
    errMsg.includes("Network connection lost") ||
    errMsg.includes("storage caused object to be reset") ||
    errMsg.includes("reset because its code was updated");
  if (nextAttempt <= 5 && isRetryableError) {
    return true;
  }
  return false;
}
```

---

## Observability and Metrics

**Docs:** https://developers.cloudflare.com/d1/observability/metrics-analytics/

D1 exposes seven key metrics with 31-day data retention:

| Metric | Field | Description |
|--------|-------|-------------|
| Read Queries (qps) | `readQueries` | Number of read queries issued |
| Write Queries (qps) | `writeQueries` | Number of write queries issued |
| Rows Read (count) | `rowsRead` | Number of rows scanned across queries |
| Rows Written (count) | `rowsWritten` | Number of rows written |
| Query Response (bytes) | `queryBatchResponseBytes` | Total response size |
| Query Latency (ms) | `queryBatchTimeMs` | Total query response time |
| Storage (bytes) | `databaseSizeBytes` | Database size |

Available via the Cloudflare dashboard (Metrics tab), GraphQL Analytics API, or `wrangler d1 insights` (experimental).

GraphQL datasets: `d1AnalyticsAdaptiveGroups`, `d1StorageAdaptiveGroups`, `d1QueriesAdaptiveGroups`.

**QueryEfficiency** = rows returned / rows read (target: approaching 1).

---

## Billing and Usage

**Docs:** https://developers.cloudflare.com/d1/observability/billing/

Tracks three billing metrics: rows read, rows written, and total storage.

Available through Cloudflare dashboard (charts with daily or month-to-date timeframes, up to 30 days history), GraphQL Analytics API, and usage-based alert notifications for rows read/written.

---

## Audit Logs

**Docs:** https://developers.cloudflare.com/d1/observability/audit-logs/

Audit logs provide a summary of changes made within your Cloudflare account. Available on all plans at no cost.

Tracked operations:
- **CreateDatabase** - new database creation
- **DeleteDatabase** - database removal
- **TimeTravel** - database version restoration

---

## Debugging and Error Handling

**Docs:** https://developers.cloudflare.com/d1/observability/debug-d1/

D1 throws Error objects when operations fail. Error prefixes:
- `D1_ERROR` - general D1 error
- `D1_EXEC_ERROR` - execution errors with line numbers
- `D1_TYPE_ERROR` - type mismatches (commonly from supplying `undefined` instead of `null`)
- `D1_COLUMN_NOTFOUND` - referenced columns do not exist

Retryable error scenarios:
- No SQL statements detected
- Account/database storage limit exceeded
- Code updates causing database resets
- Startup failures or internal storage errors
- Network connection losses
- Request stream disconnections
- Timeout errors during large writes
- Overloaded database conditions
- Memory or CPU limit exceedances

D1 automatically retries read-only queries (containing only `SELECT`, `EXPLAIN`, or `WITH`) up to two times, rolling back any writes during retries.

Debugging tools: `wrangler tail` for live logs, Cloudflare dashboard log viewer.

**Error handling example:**
```javascript
try {
    // This is an intentional misspelling
    await db.exec("INSERTZ INTO my_table (name, employees) VALUES ()");
} catch (e: any) {
    console.error({
        message: e.message
    });
}
```

**Error output:**
```json
{
  "message": "D1_EXEC_ERROR: Error in line 1: INSERTZ INTO my_table (name, employees) VALUES (): sql error: near \"INSERTZ\": syntax error in INSERTZ INTO my_table (name, employees) VALUES () at offset 0"
}
```

---

## Data Security

**Docs:** https://developers.cloudflare.com/d1/reference/data-security/

### Encryption at Rest
All stored objects (metadata, active databases, inactive databases) are automatically encrypted using AES-256 with GCM (Galois/Counter Mode). Cloudflare manages encryption keys internally.

### Encryption in Transit
Data between Workers and D1, and between network nodes and D1, uses TLS/SSL. API access via HTTP API or Wrangler is also over TLS/SSL (HTTPS).

---

## Pricing

**Docs:** https://developers.cloudflare.com/d1/platform/pricing/

D1 charges based on rows read, rows written, and storage. No data transfer or bandwidth charges.

| Metric | Workers Free | Workers Paid |
|--------|--------------|--------------|
| Rows read | 5 million/day | 25 billion/month included, then $0.001/million |
| Rows written | 100,000/day | 50 million/month included, then $1.00/million |
| Storage | 5 GB total | 5 GB included, then $0.75/GB-month |

Key details:
- Free plan limits reset daily at 00:00 UTC
- Paid plan monthly limits reset on subscription renewal dates
- Exceeding free limits blocks query execution
- Read replicas incur standard usage costs only
- Empty databases consume approximately 12 KB
- Indexes add write overhead but reduce read costs
- D1 itself does not charge for additional compute; Workers have separate billing
- Dashboard and Wrangler queries count as billable usage

---

## Limits

**Docs:** https://developers.cloudflare.com/d1/platform/limits/

| Feature | Workers Paid | Workers Free |
|---------|-------------|--------------|
| Databases per account | 50,000 | 10 |
| Maximum database size | 10 GB | 500 MB |
| Maximum storage per account | 1 TB | 5 GB |
| Time Travel duration | 30 days | 7 days |
| Max Time Travel restores | 10 per 10 minutes per database | 10 per 10 minutes per database |
| Queries per Worker invocation | 1,000 | 50 |
| Maximum columns per table | 100 | 100 |
| Maximum rows per table | Unlimited (within storage) | Unlimited (within storage) |
| Maximum string/BLOB/row size | 2 MB (2,000,000 bytes) | 2 MB |
| Maximum SQL statement length | 100 KB (100,000 bytes) | 100 KB |
| Maximum bound parameters per query | 100 | 100 |
| Maximum SQL function arguments | 32 | 32 |
| Maximum LIKE/GLOB pattern characters | 50 bytes | 50 bytes |
| Maximum bindings per Worker script | ~5,000 | ~5,000 |
| Maximum SQL query duration | 30 seconds | 30 seconds |
| Maximum file import size | 5 GB | 5 GB |
| Simultaneous connections per invocation | 6 | 6 |

### Throughput

A single D1 database is inherently single-threaded and processes queries one at a time:
- 1ms average query duration = ~1,000 queries/second
- 100ms average query duration = ~10 queries/second

Each database is backed by a single Durable Object. Read replicas are separate Durable Objects with independent throughput.

### Performance Guidelines

- Indexed `SELECT` queries: sub-millisecond SQL duration
- `INSERT`/`UPDATE` queries: several milliseconds
- Large data migrations must run in batches; avoid modifying hundreds of thousands of rows or hundreds of MBs at once
- Operations run within Workers platform CPU and memory limits

---

## Full-Text Search (FTS5)

**Docs:** https://developers.cloudflare.com/d1/sql-api/sql-statements/

D1 includes the FTS5 module for full-text search capabilities. This is a SQLite extension that enables efficient text searching across large datasets. FTS5 virtual tables cannot be exported via `wrangler d1 export` and must be deleted and recreated separately.

---

## Type Conversion

**Docs:** https://developers.cloudflare.com/d1/best-practices/query-d1/

D1 automatically converts JavaScript types to D1/SQLite types during writes. This is a permanent one-way conversion. JSON type conversions:
- JSON `null` becomes D1 `NULL`
- JSON numbers become `INTEGER` or `REAL`
- Booleans convert to integers (`true` = 1, `false` = 0)
- Objects and arrays remain as `TEXT`

---

## Wrangler Commands Reference

**Docs:** https://developers.cloudflare.com/d1/wrangler-commands/

### Database Management
| Command | Description |
|---------|-------------|
| `d1 create` | Create a new D1 database. Supports `--location` hints and `--jurisdiction` |
| `d1 info` | Get database information (size, state) |
| `d1 list` | List all D1 databases in your account |
| `d1 delete` | Remove a D1 database |

### Data Operations
| Command | Description |
|---------|-------------|
| `d1 execute` | Execute SQL commands or files (`--command` or `--file`) |
| `d1 export` | Export database as `.sql` file (`--table`, `--no-schema`, `--no-data`) |

### Time Travel
| Command | Description |
|---------|-------------|
| `d1 time-travel info` | Retrieve database info at specific timestamps |
| `d1 time-travel restore` | Restore to a point in time (bookmark or timestamp) |

### Migrations
| Command | Description |
|---------|-------------|
| `d1 migrations create` | Create versioned migration files (format: `0000_description.sql`) |
| `d1 migrations list` | Show unapplied migrations |
| `d1 migrations apply` | Apply pending migrations with automatic backups and rollback |

### Insights
| Command | Description |
|---------|-------------|
| `d1 insights` | Experimental: query performance data with customizable sorting |

All commands support `--local`, `--remote`, and `--preview` flags for targeting different execution contexts. Output can be formatted as JSON with `--json`.

---

## Horizontal Scaling Model

**Docs:** https://developers.cloudflare.com/d1/reference/faq/

D1 is designed for horizontal scale-out across multiple smaller (10 GB) databases. Applications can include thousands of databases at no extra cost. Each database is backed by a single Durable Object and is inherently single-threaded.

Key design principles:
- Shard data across multiple databases rather than scaling up a single database
- Use read replicas to increase read throughput
- Keep individual databases under 10 GB
- Optimize queries with indexes to keep query durations low
