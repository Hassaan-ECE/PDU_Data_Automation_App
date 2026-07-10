//! Canonical station identities for notifications and shared logs.

/// Stable station catalog used by Settings, Teams cards, and shared-folder layout.
/// Floor Test Station 2 was removed (not in use).
pub const KNOWN_STATIONS: &[(&str, &str)] = &[
    ("test-station-1", "Test Station 1"),
    ("test-station-3", "Test Station 3"),
    ("test-station-4", "Test Station 4"),
    ("pdu-lab", "PDU Lab"),
];

pub const DEFAULT_SUMMARY_POSTER_STATION_ID: &str = "pdu-lab";

pub fn is_known_station_id(station_id: &str) -> bool {
    let station_id = station_id.trim();
    KNOWN_STATIONS.iter().any(|(id, _)| *id == station_id)
}

pub fn station_name_for_id(station_id: &str) -> &str {
    let station_id = station_id.trim();
    if station_id.is_empty() {
        return "Unknown station";
    }
    KNOWN_STATIONS
        .iter()
        .find(|(id, _)| *id == station_id)
        .map(|(_, name)| *name)
        .unwrap_or(station_id)
}

pub fn known_station_ids() -> Vec<&'static str> {
    KNOWN_STATIONS.iter().map(|(id, _)| *id).collect()
}

pub fn known_stations_owned() -> Vec<(String, String)> {
    KNOWN_STATIONS
        .iter()
        .map(|(id, name)| ((*id).to_string(), (*name).to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_includes_floor_and_lab() {
        assert!(is_known_station_id("test-station-1"));
        assert!(is_known_station_id("pdu-lab"));
        assert_eq!(station_name_for_id("pdu-lab"), "PDU Lab");
        assert!(!is_known_station_id("unknown"));
        assert!(!is_known_station_id("test-station-2"));
        assert_eq!(KNOWN_STATIONS.len(), 4);
    }
}
