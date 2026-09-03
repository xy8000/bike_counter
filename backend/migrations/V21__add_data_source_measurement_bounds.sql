-- Source-wide earliest/latest measurement timestamps, persisted per data source.
--
-- The data-source detail page used to derive "first data from" / recency (and
-- the historical / real-time feature badges) with two DISTINCT ON queries over
-- the source's whole per-channel measurement history. PostgreSQL has no skip
-- scan for DISTINCT ON, so those queries read every measurement row of the
-- source (e.g. 20.9M rows for Hamburg), making the detail page take seconds.
--
-- Instead, the earliest and latest measurement timestamps are now maintained
-- directly on `data_sources` by the core import flow (see the DataImportService
-- / DataSourceUpdateService changes). The detail read model then reads the two
-- persisted values by primary key instead of scanning the history.
--
-- Both columns stay NULL for a source that has no measurements yet. `first`
-- only ever moves earlier and `last` only ever moves later: the import path
-- merges with LEAST/GREATEST so a later historical backfill still shrinks the
-- lower bound correctly.

ALTER TABLE data_sources ADD COLUMN first_measurement_at TIMESTAMPTZ;
ALTER TABLE data_sources ADD COLUMN last_measurement_at TIMESTAMPTZ;

-- One-time backfill of the existing history so the columns are correct before
-- the next import touches them. The scan is unavoidable for the first run (the
-- bounds are the aggregate of every stored measurement), comparable to the
-- measurements index builds already done at startup.
UPDATE data_sources ds
SET first_measurement_at = bounds.first_ts,
    last_measurement_at  = bounds.last_ts
FROM (
    SELECT s.data_source_id,
           MIN(m.timestamp) AS first_ts,
           MAX(m.timestamp) AS last_ts
    FROM measurements m
    JOIN channels c ON c.id = m.channel_id
    JOIN counting_stations s ON s.id = c.counting_station_id
    WHERE s.data_source_id IS NOT NULL
    GROUP BY s.data_source_id
) bounds
WHERE ds.id = bounds.data_source_id;
