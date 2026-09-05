//! The API_V1 station catalog: a tiny, strict parser for the `stations.yml`
//! file next to this module.
//!
//! The legacy Eco-Visio API no longer auto-discovers German counting stations
//! (they moved to the API-key-gated platform), so the operator lists the
//! counters to import in [`stations.yml`](stations.yml). Only a **fixed subset**
//! of YAML is accepted — a `stations:` block sequence of `- id:` entries with
//! optional scalar fields — so a hand-written file cannot silently mis-parse.
//! Anything unexpected is a hard parse error (startup failure).

/// The bundled catalog shipped with the API_V1 mode (see [`stations.yml`](stations.yml)).
pub const DEFAULT_STATIONS_YAML: &str = include_str!("stations.yml");

/// A counting station as listed in the catalog. Only `id` is required; the rest
/// are optional display/geometry/timezone overrides that make the station list
/// deterministic before the live metadata is fetched.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogStation {
    /// The Eco-Visio counter id (`idPdc`); also the station's external id.
    pub id: i64,
    /// Optional display name (fallback: the live metadata `titre`, else the id).
    pub name: Option<String>,
    /// Optional WGS84 latitude (decimal degrees).
    pub latitude: Option<f64>,
    /// Optional WGS84 longitude (decimal degrees).
    pub longitude: Option<f64>,
    /// Optional IANA timezone (default `Europe/Berlin`).
    pub timezone: Option<String>,
}

impl CatalogStation {
    /// The station's IANA timezone (catalog value or the German default).
    pub fn timezone(&self) -> &str {
        self.timezone.as_deref().unwrap_or("Europe/Berlin")
    }
}

/// Parses a catalog in the constrained YAML subset into ordered stations.
///
/// Accepted grammar (blank lines and `#` comments are ignored anywhere):
///
/// ```text
/// stations:
///   - id: 123
///     name: "optional display name"
///     latitude: 49.4
///     longitude: 11.0
///     timezone: Europe/Berlin
/// ```
///
/// A missing `id`, a non-integer `id`, an unknown key or a malformed scalar is a
/// parse error.
pub fn parse_catalog(yaml: &str) -> Result<Vec<CatalogStation>, String> {
    let mut stations: Vec<CatalogStation> = Vec::new();
    let mut current: Option<StationBuilder> = None;
    let mut in_stations = false;

    for (index, raw) in yaml.lines().enumerate() {
        let line = raw.trim();
        let line_number = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !in_stations {
            if line == "stations:" {
                in_stations = true;
                continue;
            }
            return Err(format!(
                "catalog line {line_number}: expected 'stations:', got '{line}'"
            ));
        }

        if let Some(rest) = line.strip_prefix("- ") {
            if let Some(builder) = current.take() {
                stations.push(builder.finish()?);
            }
            // Accept both `- id: 123` (mapping form) and `- 123` (list form).
            let id = match rest.split_once(':') {
                Some((key, value)) if key.trim() == "id" => parse_i64(value, line_number, "id")?,
                Some((key, _)) => {
                    return Err(format!(
                        "catalog line {line_number}: unexpected key '{key}' in a station entry; \
                         expected 'id'"
                    ));
                }
                None => parse_i64(rest, line_number, "id")?,
            };
            current = Some(StationBuilder {
                id,
                ..StationBuilder::default()
            });
            continue;
        }

        // A scalar field `key: value` (only inside a station entry).
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!(
                "catalog line {line_number}: expected '- id:' or 'key: value', got '{line}'"
            ));
        };
        let key = key.trim();
        let value = value.trim();
        let builder = current.as_mut().ok_or_else(|| {
            format!("catalog line {line_number}: field '{key}' appears before any '- id:' entry")
        })?;
        match key {
            "name" => builder.name = Some(unquote(value).to_string()),
            "latitude" => builder.latitude = Some(parse_f64(value, line_number, "latitude")?),
            "longitude" => builder.longitude = Some(parse_f64(value, line_number, "longitude")?),
            "timezone" => builder.timezone = Some(unquote(value).to_string()),
            other => {
                return Err(format!(
                    "catalog line {line_number}: unknown field '{other}' (allowed: id, name, \
                     latitude, longitude, timezone)"
                ));
            }
        }
    }

    if let Some(builder) = current.take() {
        stations.push(builder.finish()?);
    }
    if stations.is_empty() {
        return Err("catalog contains no stations under 'stations:'".to_string());
    }
    Ok(stations)
}

/// Partially parsed station entry.
#[derive(Default)]
struct StationBuilder {
    id: i64,
    name: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    timezone: Option<String>,
}

impl StationBuilder {
    fn finish(self) -> Result<CatalogStation, String> {
        Ok(CatalogStation {
            id: self.id,
            name: self.name,
            latitude: self.latitude,
            longitude: self.longitude,
            timezone: self.timezone,
        })
    }
}

/// Strips one level of surrounding single/double quotes, if present.
fn unquote(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn parse_i64(value: &str, line: usize, key: &str) -> Result<i64, String> {
    value.trim().parse::<i64>().map_err(|_| {
        format!(
            "catalog line {line}: '{key}' is not an integer: '{}'",
            value.trim()
        )
    })
}

fn parse_f64(value: &str, line: usize, key: &str) -> Result<f64, String> {
    value.trim().parse::<f64>().map_err(|_| {
        format!(
            "catalog line {line}: '{key}' is not a number: '{}'",
            value.trim()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
        # a comment
        stations:
          - id: 100063085
            name: "Stadt Stein Nürnberger Straße"
            latitude: 49.4163
            longitude: 11.0188
            timezone: Europe/Berlin
          - id: 100000445
            name: 'Sabrina Footbridge'
    "#;

    #[test]
    fn parses_the_bundled_catalog() {
        let stations = parse_catalog(DEFAULT_STATIONS_YAML).unwrap();
        assert_eq!(stations.len(), 5);
        assert!(stations.iter().any(|s| s.id == 100063085));
        assert!(stations.iter().all(|s| s.timezone() == "Europe/Berlin"));
    }

    #[test]
    fn parses_multiple_stations_and_quoted_names() {
        let stations = parse_catalog(VALID).unwrap();
        assert_eq!(stations.len(), 2);
        assert_eq!(stations[0].id, 100063085);
        assert_eq!(
            stations[0].name.as_deref(),
            Some("Stadt Stein Nürnberger Straße")
        );
        assert_eq!(stations[0].latitude, Some(49.4163));
        assert_eq!(stations[0].longitude, Some(11.0188));
        assert_eq!(stations[0].timezone(), "Europe/Berlin");
        assert_eq!(stations[1].id, 100000445);
        assert_eq!(stations[1].name.as_deref(), Some("Sabrina Footbridge"));
        assert_eq!(stations[1].timezone(), "Europe/Berlin", "timezone defaults");
    }

    #[test]
    fn rejects_non_integer_id() {
        let err = parse_catalog("stations:\n  - id: not-a-number\n").unwrap_err();
        assert!(err.contains("not an integer"), "got: {err}");
    }

    #[test]
    fn rejects_missing_id() {
        // A station entry that does not start with an `id` is rejected.
        let err = parse_catalog("stations:\n  - name: foo\n").unwrap_err();
        assert!(err.contains("expected 'id'"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_key() {
        let err = parse_catalog("stations:\n  - id: 1\n    foo: bar\n").unwrap_err();
        assert!(err.contains("unknown field 'foo'"), "got: {err}");
    }

    #[test]
    fn rejects_missing_stations_header() {
        let err = parse_catalog("- id: 1\n").unwrap_err();
        assert!(err.contains("expected 'stations:'"), "got: {err}");
    }

    #[test]
    fn rejects_empty_catalog() {
        let err = parse_catalog("stations:\n").unwrap_err();
        assert!(err.contains("no stations"), "got: {err}");
    }
}
