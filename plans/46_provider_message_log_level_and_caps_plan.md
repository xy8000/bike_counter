# 46 - Provider-message log level, filtering and DB cap

Status: implemented

## Problem

The Münster import spams a large number of `WARNING` provider messages into the
`data_source_provider_messages` table. Every monthly CSV that lacks a queried
channel column emits a full "channel ... has no column in ..." warning
([`parsing.rs`](../backend/src/adapter/driven/muenster_github/parsing.rs:150)),
and nothing bounds how many messages a single data source can accumulate. The
result is a bloated messages list and a noisy database.

## Goals

1. Reclassify the Münster missing-column event from `WARNING` to `DEBUG`.
2. Introduce a per-provider `log_level` in the `[data_sources.provider]`
   configuration section (default `WARNING`).
3. Make the **core** drop provider messages below the configured `log_level`.
4. Make the **core** persist only the first **1000** messages per data source,
   adding one truncation `WARNING` (and printing it to stdout) when exceeded.
5. Make the **database** guarantee at most **1001** messages per data source.
6. Trim existing messages to the newest 1000 per data source (data loss accepted).
7. Add `CONTRIBUTING.md` with an adapter implementation guide and shorten
   [`plans/README.md`](../plans/README.md:1).

## Decisions / assumptions

- **Severity order**: `TRACE < DEBUG < INFO < WARNING < ERROR`. "Exceed the
  level" means "below the configured threshold" (more verbose), so a message is
  dropped when its severity is lower than `log_level`. With the default
  `WARNING`, `INFO`/`DEBUG`/`TRACE` are dropped and `WARNING`/`ERROR` persist.
- **`log_level` is a dedicated key** in `[data_sources.provider]` (not a
  `vars` entry), stored on the domain `DataProviderConfiguration` and validated
  at configuration-parse time via `ProviderMessageSeverity::from_str`.
- **Filtering lives in the core**: a new `FilteringProviderMessageSink` (domain
  implementation of the existing `ProviderMessageSink` port) wraps the scoped
  adapter sink. The core enforces both the level and the cap, so the adapter
  (and the database) stay policy-free.
- **Cap is per data source** (the sink is already scoped per data source). The
  core persists the first 1000 events; the 1001st event is dropped and instead
  emits exactly one `WARNING` ("events truncated") directly through the inner
  sink plus a `println!` to stdout. Further events are silently dropped. The
  truncation warning is not counted against the 1000, yielding at most 1001 rows.
- **DB safeguard**: an `AFTER INSERT` trigger deletes rows beyond the newest
  1001 per data source, as defense in depth in case the core cap is bypassed.
- **Existing-data cleanup** (confirmed): keep the newest 1000 messages per data
  source and delete the older ones. The +1 truncation warning is only added by
  future imports. Data loss for messages is acceptable.

## Data model / migration

New migration `V13__cap_data_source_provider_messages.sql`:

1. One-time cleanup — delete everything except the newest 1000 per data source
   (ordered by `occurred_at DESC, id DESC`).
2. A trigger function + `AFTER INSERT` trigger that deletes rows beyond the
   newest 1001 per data source after each insert.

```sql
-- Keep only the newest 1000 provider messages per data source.
DELETE FROM data_source_provider_messages AS m
USING (
    SELECT id
    FROM (
        SELECT
            id,
            row_number() OVER (
                PARTITION BY data_source_id
                ORDER BY occurred_at DESC, id DESC
            ) AS rn
        FROM data_source_provider_messages
    ) AS ranked
    WHERE ranked.rn > 1000
) AS excess
WHERE m.id = excess.id;

-- Defense-in-depth cap: at most 1001 messages per data source.
CREATE OR REPLACE FUNCTION enforce_data_source_provider_message_cap()
RETURNS trigger AS $$
BEGIN
    DELETE FROM data_source_provider_messages
    WHERE data_source_id = NEW.data_source_id
      AND id IN (
          SELECT id
          FROM data_source_provider_messages
          WHERE data_source_id = NEW.data_source_id
          ORDER BY occurred_at DESC, id DESC
          OFFSET 1001
      );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_data_source_provider_message_cap
AFTER INSERT ON data_source_provider_messages
FOR EACH ROW EXECUTE FUNCTION enforce_data_source_provider_message_cap();
```

## Core architecture

### Severity ordering

In [`provider_message.rs`](../backend/src/core/domain/data_source/provider_message.rs:20)
add a rank (`Trace = 0` .. `Error = 4`) and an
`at_or_above(threshold: ProviderMessageSeverity) -> bool` helper used by the
filter.

### Configuration

In
[`configuration.rs`](../backend/src/core/domain/configuration/configuration.rs:282)
add `log_level: ProviderMessageSeverity` to `DataProviderConfiguration`:

- keep `new(provider_type, vars)` (defaults `log_level` to `Warning`),
- add `with_log_level(self, level) -> Self` and
  `log_level() -> ProviderMessageSeverity`.

In
[`configuration_toml_adapter.rs`](../backend/src/adapter/driven/configuration_toml_adapter.rs:48)
extend `DataProviderDto` with `#[serde(default = "default_log_level")] log_level: String`,
parse it with `ProviderMessageSeverity::from_str` (mapping failures to
`ConfigError::InvalidFormat`) and pass it via `with_log_level`.

### Filtering sink

New module
[`provider_message_filter.rs`](../backend/src/core/domain/data_source/mod.rs:16)
(exact path `src/core/domain/data_source/provider_message_filter.rs`):

```text
FilteringProviderMessageSink {
    inner: Arc<dyn ProviderMessageSink + Send + Sync>,
    min_level: ProviderMessageSeverity,
    max_messages: usize,               // 1000
    recorded: AtomicUsize,
    reported: AtomicBool,
}
```

`provider_event_occurred(severity, message)`:

1. if `!severity.at_or_above(self.min_level)` -> drop;
2. `let n = self.recorded.fetch_add(1, SeqCst);`
   - `n < 1000` -> forward to `inner`;
   - `n >= 1000` -> drop; if `reported` was false (CAS true), emit one
     `WARNING` through `inner` and `println!` the truncation notice.

### Wiring

In [`startup_service.rs`](../backend/src/core/application/startup_service.rs:104)
wrap the scoped sink before attaching:

```rust
let scoped = self.provider_message_sink_factory.scoped(data_source_id);
let filtered = Arc::new(FilteringProviderMessageSink::new(
    scoped,
    data_source.provider().log_level(),
    MAX_PROVIDER_MESSAGES,
));
provider.attach_provider_messages(filtered);
```

## File changes

- `migrations/V13__cap_data_source_provider_messages.sql` — cleanup + trigger.
- `src/core/domain/data_source/provider_message.rs` — rank + `at_or_above`.
- `src/core/domain/data_source/provider_message_filter.rs` — new filter sink.
- `src/core/domain/data_source/mod.rs` — register the new module.
- `src/core/domain/configuration/configuration.rs` — `log_level` field/accessors.
- `src/core/domain/configuration/error.rs` — no change (reuse `InvalidFormat`).
- `src/adapter/driven/configuration_toml_adapter.rs` — parse `log_level`.
- `src/adapter/driven/muenster_github/parsing.rs` — `Warning` -> `Debug`.
- `src/adapter/driven/muenster_github/tests.rs` — update severity assertion.
- `src/core/application/startup_service.rs` — wrap the sink with the filter.
- `config.toml.example` — document `log_level = "WARNING"`.
- `README.md` — data-provider messages section + config snippet.
- `CONTRIBUTING.md` — new adapter implementation guide.
- `plans/README.md` — register this plan and shorten summaries.

## Testing / gates

- Unit-test `FilteringProviderMessageSink`: drops below `min_level`; persists up
  to 1000; drops the 1001st+; emits exactly one truncation `WARNING`.
- Configuration tests: default `log_level`, explicit parse, invalid value error.
- Update the Münster test
  [`missing_channel_column_emits_warning_and_returns_empty_batch`](../backend/src/adapter/driven/muenster_github/tests.rs:799)
  to expect `DEBUG` (rename accordingly).
- Postgres repository tests still pass with the trigger present.
- Run `make check`, `make test-rest`/`make test`, `make coverage`.

## Definition of done

- [x] Missing-column event reclassified to `DEBUG`
- [x] Per-provider `log_level` parsed with default `WARNING`
- [x] Core filters by level and caps at 1000 + one truncation warning (stdout)
- [x] Migration trims existing rows and adds the 1001-row trigger
- [x] `CONTRIBUTING.md` added
- [x] `make check`, `make test`/`make test-rest`, `make coverage` green
