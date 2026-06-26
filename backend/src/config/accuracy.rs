use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::resource_paths;

const ACCURACY_THRESHOLDS_FILE_NAME: &str = "pdu500.accuracy-thresholds.json";
const DEFAULT_ACCURACY_THRESHOLDS_JSON: &str =
    include_str!("../../../config/report-layouts/pdu500.accuracy-thresholds.json");

#[derive(Debug, Error)]
pub enum AccuracyThresholdError {
    #[error("accuracy threshold config could not be read from {path}: {source}")]
    ReadFailed {
        path: String,
        source: std::io::Error,
    },
    #[error("accuracy threshold config JSON is invalid in {path}: {source}")]
    InvalidJson {
        path: String,
        source: serde_json::Error,
    },
    #[error("accuracy threshold config is invalid in {path}: {message}")]
    InvalidValue { path: String, message: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccuracyThresholdConfig {
    pub schema_version: u16,
    pub profile_id: String,
    pub display_name: String,
    #[serde(default)]
    pub notes: Vec<String>,
    pub system: SystemAccuracyThresholds,
    pub breaker: BreakerAccuracyThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemAccuracyThresholds {
    #[serde(rename = "100_percent")]
    pub full_load: SystemMetricThresholds,
    #[serde(rename = "50_percent")]
    pub half_load: SystemMetricThresholds,
    #[serde(rename = "20_percent")]
    pub low_load: SystemMetricThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemMetricThresholds {
    pub active_power: f64,
    pub apparent_power: f64,
    pub power_factor: f64,
    pub voltage: f64,
    pub current: f64,
}

#[derive(Debug, Clone)]
pub struct BreakerAccuracyThresholds {
    pub full_load: BreakerMetricThresholds,
    pub half_load: BreakerMetricThresholds,
    pub low_load: BreakerMetricThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BreakerMetricThresholds {
    pub voltage: f64,
    pub current: f64,
    pub active_power: f64,
    pub power_factor: f64,
}

#[derive(Debug, Deserialize)]
struct RawBreakerAccuracyThresholds {
    #[serde(rename = "100_percent")]
    full_load: Option<BreakerMetricThresholds>,
    #[serde(rename = "50_percent")]
    half_load: Option<BreakerMetricThresholds>,
    #[serde(rename = "20_percent")]
    low_load: Option<BreakerMetricThresholds>,
    #[serde(default)]
    all_loads: Option<BreakerMetricThresholds>,
}

impl<'de> Deserialize<'de> for BreakerAccuracyThresholds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawBreakerAccuracyThresholds::deserialize(deserializer)?;
        let legacy_all_loads = raw.all_loads;
        let full_load = raw
            .full_load
            .or_else(|| legacy_all_loads.clone())
            .ok_or_else(|| <D::Error as serde::de::Error>::missing_field("breaker.100_percent"))?;
        let half_load = raw
            .half_load
            .or_else(|| legacy_all_loads.clone())
            .ok_or_else(|| <D::Error as serde::de::Error>::missing_field("breaker.50_percent"))?;
        let low_load = raw
            .low_load
            .or(legacy_all_loads)
            .ok_or_else(|| <D::Error as serde::de::Error>::missing_field("breaker.20_percent"))?;

        Ok(Self {
            full_load,
            half_load,
            low_load,
        })
    }
}

impl AccuracyThresholdConfig {
    fn from_json(path: String, json: &str) -> Result<Self, AccuracyThresholdError> {
        let config = serde_json::from_str::<Self>(json).map_err(|source| {
            AccuracyThresholdError::InvalidJson {
                path: path.clone(),
                source,
            }
        })?;

        config.validate(&path)?;
        Ok(config)
    }

    fn validate(&self, path: &str) -> Result<(), AccuracyThresholdError> {
        if self.schema_version != 1 {
            return Err(AccuracyThresholdError::InvalidValue {
                path: path.to_string(),
                message: format!(
                    "unsupported schema_version {}; expected 1",
                    self.schema_version
                ),
            });
        }

        if self.profile_id.trim().is_empty() {
            return Err(invalid_value(path, "profile_id must not be empty"));
        }

        if self.display_name.trim().is_empty() {
            return Err(invalid_value(path, "display_name must not be empty"));
        }

        validate_system_thresholds(path, "system.100_percent", &self.system.full_load)?;
        validate_system_thresholds(path, "system.50_percent", &self.system.half_load)?;
        validate_system_thresholds(path, "system.20_percent", &self.system.low_load)?;
        validate_breaker_thresholds(path, "breaker.100_percent", &self.breaker.full_load)?;
        validate_breaker_thresholds(path, "breaker.50_percent", &self.breaker.half_load)?;
        validate_breaker_thresholds(path, "breaker.20_percent", &self.breaker.low_load)?;

        Ok(())
    }
}

pub fn load_accuracy_thresholds() -> Result<AccuracyThresholdConfig, AccuracyThresholdError> {
    if let Some(path) = configured_threshold_path() {
        return load_from_path(path);
    }

    load_accuracy_thresholds_from_candidates(candidate_threshold_paths())
}

fn load_accuracy_thresholds_from_candidates(
    candidate_paths: Vec<PathBuf>,
) -> Result<AccuracyThresholdConfig, AccuracyThresholdError> {
    for path in candidate_paths {
        if path.is_file() {
            return load_from_path(path);
        }
    }

    AccuracyThresholdConfig::from_json(
        "built-in defaults".to_string(),
        DEFAULT_ACCURACY_THRESHOLDS_JSON,
    )
}

fn configured_threshold_path() -> Option<PathBuf> {
    std::env::var_os("PDU_ACCURACY_THRESHOLDS_PATH").map(PathBuf::from)
}

fn candidate_threshold_paths() -> Vec<PathBuf> {
    resource_paths::report_layout_candidate_paths(ACCURACY_THRESHOLDS_FILE_NAME)
}

#[cfg(test)]
fn load_accuracy_thresholds_with_resource_dir(
    resource_dir: &Path,
) -> Result<AccuracyThresholdConfig, AccuracyThresholdError> {
    load_accuracy_thresholds_from_candidates(resource_paths::report_layout_candidate_paths_for(
        ACCURACY_THRESHOLDS_FILE_NAME,
        Some(resource_dir),
    ))
}

fn load_from_path(path: PathBuf) -> Result<AccuracyThresholdConfig, AccuracyThresholdError> {
    let display_path = path.display().to_string();
    let json = fs::read_to_string(&path).map_err(|source| AccuracyThresholdError::ReadFailed {
        path: display_path.clone(),
        source,
    })?;

    AccuracyThresholdConfig::from_json(display_path, &json)
}

fn validate_system_thresholds(
    path: &str,
    prefix: &str,
    thresholds: &SystemMetricThresholds,
) -> Result<(), AccuracyThresholdError> {
    validate_threshold(
        path,
        &format!("{prefix}.active_power"),
        thresholds.active_power,
    )?;
    validate_threshold(
        path,
        &format!("{prefix}.apparent_power"),
        thresholds.apparent_power,
    )?;
    validate_threshold(
        path,
        &format!("{prefix}.power_factor"),
        thresholds.power_factor,
    )?;
    validate_threshold(path, &format!("{prefix}.voltage"), thresholds.voltage)?;
    validate_threshold(path, &format!("{prefix}.current"), thresholds.current)
}

fn validate_breaker_thresholds(
    path: &str,
    prefix: &str,
    thresholds: &BreakerMetricThresholds,
) -> Result<(), AccuracyThresholdError> {
    validate_threshold(path, &format!("{prefix}.voltage"), thresholds.voltage)?;
    validate_threshold(path, &format!("{prefix}.current"), thresholds.current)?;
    validate_threshold(
        path,
        &format!("{prefix}.active_power"),
        thresholds.active_power,
    )?;
    validate_threshold(
        path,
        &format!("{prefix}.power_factor"),
        thresholds.power_factor,
    )
}

fn validate_threshold(path: &str, name: &str, value: f64) -> Result<(), AccuracyThresholdError> {
    if value.is_finite() && value >= 0.0 {
        return Ok(());
    }

    Err(invalid_value(
        path,
        &format!("{name} must be a finite number greater than or equal to 0"),
    ))
}

fn invalid_value(path: &str, message: &str) -> AccuracyThresholdError {
    AccuracyThresholdError::InvalidValue {
        path: path.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn default_accuracy_thresholds_load() {
        let config = AccuracyThresholdConfig::from_json(
            "test defaults".to_string(),
            DEFAULT_ACCURACY_THRESHOLDS_JSON,
        )
        .expect("default thresholds should parse");

        assert_eq!(config.system.full_load.current, 0.3);
        assert_eq!(config.system.half_load.current, 0.39);
        assert_eq!(config.system.low_load.active_power, 0.75);
        assert_eq!(config.breaker.full_load.power_factor, 2.0);
        assert_eq!(config.breaker.half_load.current, 0.39);
        assert_eq!(config.breaker.low_load.active_power, 0.75);
    }

    #[test]
    fn legacy_breaker_all_loads_config_is_still_accepted() {
        let mut value: serde_json::Value =
            serde_json::from_str(DEFAULT_ACCURACY_THRESHOLDS_JSON).expect("valid JSON");
        value["breaker"] = serde_json::json!({
            "all_loads": {
                "voltage": 0.3,
                "current": 0.3,
                "active_power": 0.6,
                "power_factor": 2.0
            }
        });
        let json = serde_json::to_string(&value).expect("JSON should serialize");

        let config = AccuracyThresholdConfig::from_json("legacy.json".to_string(), &json)
            .expect("legacy all_loads thresholds should parse");

        assert_eq!(config.breaker.full_load.current, 0.3);
        assert_eq!(config.breaker.half_load.current, 0.3);
        assert_eq!(config.breaker.low_load.active_power, 0.6);
    }

    #[test]
    fn load_with_resource_dir_uses_bundled_thresholds_before_builtin() {
        let temp = TempDir::new().expect("temp dir");
        let resource_threshold_dir = temp
            .path()
            .join("_up_")
            .join("config")
            .join("report-layouts");
        fs::create_dir_all(&resource_threshold_dir).expect("create resource layout dir");
        let resource_json = DEFAULT_ACCURACY_THRESHOLDS_JSON
            .replace(
                r#""profile_id": "pdu500.rev02.accuracy-thresholds""#,
                r#""profile_id": "pdu500.rev02.accuracy-resource-test""#,
            )
            .replace(r#""current": 0.3"#, r#""current": 9.99"#);
        fs::write(
            resource_threshold_dir.join(ACCURACY_THRESHOLDS_FILE_NAME),
            resource_json,
        )
        .expect("write resource thresholds");

        let config = load_accuracy_thresholds_with_resource_dir(temp.path())
            .expect("resource thresholds load");

        assert_eq!(config.profile_id, "pdu500.rev02.accuracy-resource-test");
        assert_eq!(config.system.full_load.current, 9.99);
    }

    #[test]
    fn negative_threshold_is_rejected() {
        let json =
            DEFAULT_ACCURACY_THRESHOLDS_JSON.replace("\"current\": 0.3", "\"current\": -0.3");
        let error = AccuracyThresholdConfig::from_json("bad.json".to_string(), &json)
            .expect_err("negative thresholds should fail validation");

        assert!(error.to_string().contains("greater than or equal to 0"));
    }
}
