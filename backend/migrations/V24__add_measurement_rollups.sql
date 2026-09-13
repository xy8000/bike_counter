-- Pre-aggregated hourly and daily rollups over the raw measurements, keyed by
-- the channel's own counting-station local calendar bucket. The analytics read
-- model sums these instead of scanning the raw history for coarse windows
-- (overview metrics, monthly bar, 30-day/year graphs, hour-of-day radars).
--
-- `local_date` / `local_hour` are computed in the channel's own station timezone
-- (each channel belongs to exactly one counting station with one timezone), which
-- matches the analytics' DST-aware local-day/local-hour semantics.

CREATE TABLE measurement_hourly (
    channel_id         UUID     NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    resolution_seconds BIGINT   NOT NULL,
    local_date         DATE     NOT NULL,
    local_hour         SMALLINT NOT NULL,
    total              BIGINT   NOT NULL,
    PRIMARY KEY (channel_id, resolution_seconds, local_date, local_hour)
);

CREATE TABLE measurement_daily (
    channel_id         UUID   NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    resolution_seconds BIGINT NOT NULL,
    local_date         DATE   NOT NULL,
    total              BIGINT NOT NULL,
    PRIMARY KEY (channel_id, resolution_seconds, local_date)
);

-- Date-first indexes for the read path, which filters a channel list by a local
-- date range and then groups by month or hour-of-day.
CREATE INDEX measurement_hourly_date_resolution_idx
    ON measurement_hourly (local_date, resolution_seconds);
CREATE INDEX measurement_daily_date_resolution_idx
    ON measurement_daily (local_date, resolution_seconds);

-- One-time backfill of the existing history. The scan is unavoidable for the
-- first run (comparable to the measurement index builds already done at startup).
INSERT INTO measurement_hourly (channel_id, resolution_seconds, local_date, local_hour, total)
SELECT m.channel_id,
       m.resolution_seconds,
       (m.timestamp AT TIME ZONE s.timezone)::date AS local_date,
       EXTRACT(HOUR FROM (m.timestamp AT TIME ZONE s.timezone))::smallint AS local_hour,
       SUM(m.value)::bigint AS total
FROM measurements m
JOIN channels c          ON c.id = m.channel_id
JOIN counting_stations s ON s.id = c.counting_station_id
GROUP BY m.channel_id, m.resolution_seconds,
         (m.timestamp AT TIME ZONE s.timezone)::date,
         EXTRACT(HOUR FROM (m.timestamp AT TIME ZONE s.timezone));

INSERT INTO measurement_daily (channel_id, resolution_seconds, local_date, total)
SELECT m.channel_id,
       m.resolution_seconds,
       (m.timestamp AT TIME ZONE s.timezone)::date AS local_date,
       SUM(m.value)::bigint AS total
FROM measurements m
JOIN channels c          ON c.id = m.channel_id
JOIN counting_stations s ON s.id = c.counting_station_id
GROUP BY m.channel_id, m.resolution_seconds,
         (m.timestamp AT TIME ZONE s.timezone)::date;
