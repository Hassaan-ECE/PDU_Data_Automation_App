use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

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

#[derive(Debug, Clone, Deserialize)]
pub struct BreakerAccuracyThresholds {
    pub all_loads: BreakerMetricThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BreakerMetricThresholds {
    pub voltage: f64,
    pub current: f64,
    pub active_power: f64,
    pub power_factor: f64,
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
        validate_threshold(
            path,
            "breaker.all_loads.voltage",
            self.breaker.all_loads.voltage,
        )?;
        validate_threshold(
            path,
            "breaker.all_loads.current",
            self.breaker.all_loads.current,
        )?;
        validate_threshold(
            path,
            "breaker.all_loads.active_power",
            self.breaker.all_loads.active_power,
        )?;
        validate_threshold(
            path,
            "breaker.all_loads.power_factor",
            self.breaker.all_loads.power_factor,
        )?;

        Ok(())
    }
}

pub fn load_accuracy_thresholds() -> Result<AccuracyThresholdConfig, AccuracyThresholdError> {
    if let Some(path) = configured_threshold_path() {
        return load_from_path(path);
    }

    for path in candidate_threshold_paths() {
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
    let mut paths = Vec::new();

    if let Ok(current_dir) = std::env::current_dir() {
        paths.push(
            current_dir
                .join("config")
                .join("report-layouts")
                .join(ACCURACY_THRESHOLDS_FILE_NAME),
        );
        paths.push(
            current_dir
                .join("..")
                .join("config")
                .join("report-layouts")
                .join(ACCURACY_THRESHOLDS_FILE_NAME),
        );
    }

    paths.push(
        PathBuf::from("C:/PDU500")
            .join("config")
            .join("report-layouts")
            .join(ACCURACY_THRESHOLDS_FILE_NAME),
    );

    paths
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
        assert_eq!(config.breaker.all_loads.power_factor, 2.0);
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
