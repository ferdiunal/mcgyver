-- One row per installation, as of its latest ping. Times are ISO-8601 UTC.
CREATE TABLE IF NOT EXISTS machines (
  machine TEXT PRIMARY KEY,
  first_seen TEXT,
  last_seen TEXT,
  version TEXT,
  os TEXT,
  os_version TEXT,
  arch TEXT,
  lang TEXT,
  ip TEXT,
  country TEXT,
  pings INTEGER
);

-- Who was around on which UTC day, and on which version: the daily actives.
CREATE TABLE IF NOT EXISTS days (
  day TEXT,
  machine TEXT,
  version TEXT,
  PRIMARY KEY (day, machine)
);

-- For "seen in the last N days".
CREATE INDEX IF NOT EXISTS machines_last_seen ON machines (last_seen);
