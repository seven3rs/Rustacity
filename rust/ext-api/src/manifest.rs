// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The manifest: what an extension declares about itself, and what the host
//! refuses to accept.
//!
//! Parsing is deliberately strict and the messages are deliberately specific —
//! an extension author who mistypes `capabilites` should be told, not handed an
//! extension that quietly cannot read audio.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::abi;
use crate::error::ExtError;

/// The file an extension keeps its manifest in.
pub const MANIFEST_FILE: &str = "extension.toml";
/// The file an extension keeps its module in.
pub const MODULE_FILE: &str = "extension.wasm";

/// A manifest that has passed every rule of API version 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// The API version the extension was written against. Always 1 here.
    pub api_version: i32,
    /// Reverse-DNS identifier, unique among installed extensions.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The extension's own version, `MAJOR.MINOR.PATCH`.
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    /// Capabilities the extension declares, in the order given.
    pub capabilities: Vec<String>,
    /// Parameters, in the order `rx_set_param` indexes them.
    pub parameters: Vec<Parameter>,
}

/// One parameter, in manifest order.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub id: String,
    pub name: String,
    /// Position in the manifest, which is the index `rx_set_param` takes.
    pub index: usize,
    pub kind: ParameterKind,
    /// Default as the ABI carries it: a float, 0/1 for a bool, an index for a
    /// choice.
    pub default: f32,
    pub unit: Option<String>,
    pub description: Option<String>,
}

/// What a parameter is, and the values it may take.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterKind {
    /// A number in `min..=max`.
    Float { min: f64, max: f64 },
    /// `0.0` or `1.0`.
    Bool,
    /// An index into `choices`.
    Choice { choices: Vec<String> },
}

impl ParameterKind {
    /// The word the manifest uses for this kind.
    pub fn name(&self) -> &'static str {
        match self {
            ParameterKind::Float { .. } => "float",
            ParameterKind::Bool => "bool",
            ParameterKind::Choice { .. } => "choice",
        }
    }
}

impl Parameter {
    /// Turns text from a command line or a text field into the value the ABI
    /// carries, checking it against this parameter.
    ///
    /// A bool takes `true`/`false` and the usual spellings of both; a choice
    /// takes its index or one of its names.
    pub fn parse_value(&self, text: &str) -> Result<f32, ExtError> {
        let text = text.trim();
        match &self.kind {
            ParameterKind::Float { min, max } => {
                let value: f64 = text.parse().map_err(|_| {
                    ExtError::InvalidArgument(format!(
                        "'{text}' is not a number for parameter '{}'",
                        self.id
                    ))
                })?;
                if value < *min || value > *max {
                    return Err(ExtError::InvalidArgument(format!(
                        "{} is outside the range of '{}' ({}..{})",
                        value, self.id, min, max
                    )));
                }
                Ok(value as f32)
            }
            ParameterKind::Bool => match text.to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => Ok(1.0),
                "false" | "no" | "off" | "0" => Ok(0.0),
                _ => Err(ExtError::InvalidArgument(format!(
                    "'{text}' is not true or false for parameter '{}'",
                    self.id
                ))),
            },
            ParameterKind::Choice { choices } => {
                if let Ok(index) = text.parse::<usize>() {
                    if index < choices.len() {
                        return Ok(index as f32);
                    }
                    return Err(ExtError::InvalidArgument(format!(
                        "choice {index} is out of range for '{}' (0..{})",
                        self.id,
                        choices.len() - 1
                    )));
                }
                match choices
                    .iter()
                    .position(|choice| choice.eq_ignore_ascii_case(text))
                {
                    Some(index) => Ok(index as f32),
                    None => Err(ExtError::InvalidArgument(format!(
                        "'{text}' is not one of the choices of '{}': {}",
                        self.id,
                        choices.join(", ")
                    ))),
                }
            }
        }
    }
}

impl Manifest {
    /// Where an extension's manifest lives, given its directory.
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(MANIFEST_FILE)
    }

    /// Where an extension's module lives, given its directory.
    pub fn module_path_in(dir: &Path) -> PathBuf {
        dir.join(MODULE_FILE)
    }

    /// Reads and validates `extension.toml` from an extension directory.
    pub fn load(dir: &Path) -> Result<Self, ExtError> {
        let path = Self::path_in(dir);
        let text = std::fs::read_to_string(&path).map_err(|error| {
            ExtError::Io(format!("cannot read {}: {error}", path.display()))
        })?;
        Self::from_str(&text).map_err(|error| match error {
            // Point at the file, not just at the rule that was broken.
            ExtError::ManifestInvalid(message) => {
                ExtError::ManifestInvalid(format!("{}: {message}", path.display()))
            }
            other => other,
        })
    }

    /// Validates manifest text.
    pub fn from_str(text: &str) -> Result<Self, ExtError> {
        let raw: RawManifest = toml::from_str(text)
            .map_err(|error| ExtError::ManifestInvalid(format!("{error}")))?;
        Self::from_raw(raw)
    }

    /// The parameter with this id, if any.
    pub fn parameter(&self, id: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|parameter| parameter.id == id)
    }

    /// Whether the manifest declares this capability.
    pub fn has_capability(&self, name: &str) -> bool {
        self.capabilities.iter().any(|held| held == name)
    }

    /// Whether the extension declared everything it needs to process audio.
    ///
    /// API version 1 offers nothing else to do, so an extension that withholds
    /// either half of the audio contract has no instance to give it.
    pub fn can_process_audio(&self) -> bool {
        self.has_capability(abi::CAP_AUDIO_READ) && self.has_capability(abi::CAP_AUDIO_WRITE)
    }

    fn from_raw(raw: RawManifest) -> Result<Self, ExtError> {
        if raw.api_version != abi::API_VERSION {
            return Err(ExtError::ManifestInvalid(format!(
                "api_version {} is not supported; this host speaks version {}",
                raw.api_version,
                abi::API_VERSION
            )));
        }
        validate_id(&raw.id)?;
        if raw.name.trim().is_empty() {
            return Err(ExtError::ManifestInvalid(
                "name must not be empty".to_string(),
            ));
        }
        validate_version(&raw.version)?;
        validate_capabilities(&raw.capabilities)?;

        let mut parameters: Vec<Parameter> = Vec::with_capacity(raw.parameters.len());
        for (index, raw_parameter) in raw.parameters.iter().enumerate() {
            if parameters
                .iter()
                .any(|known| known.id == raw_parameter.id)
            {
                return Err(ExtError::ManifestInvalid(format!(
                    "duplicate parameter id '{}'",
                    raw_parameter.id
                )));
            }
            parameters.push(raw_parameter.validate(index)?);
        }

        Ok(Manifest {
            api_version: raw.api_version,
            id: raw.id,
            name: raw.name,
            version: raw.version,
            description: raw.description,
            author: raw.author,
            license: raw.license,
            capabilities: raw.capabilities,
            parameters,
        })
    }
}

// ---------------------------------------------------------------------------
// The raw shape: what the file says, before any rule is applied.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    api_version: i32,
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    parameters: Vec<RawParameter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawParameter {
    id: String,
    name: String,
    kind: String,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    default: Option<toml::Value>,
    #[serde(default)]
    choices: Option<Vec<String>>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

impl RawParameter {
    fn validate(&self, index: usize) -> Result<Parameter, ExtError> {
        if self.id.trim().is_empty() {
            return Err(ExtError::ManifestInvalid(format!(
                "parameter {index} needs an id"
            )));
        }
        if self.name.trim().is_empty() {
            return Err(ExtError::ManifestInvalid(format!(
                "parameter '{}' needs a name",
                self.id
            )));
        }

        let kind = match self.kind.as_str() {
            "float" => {
                let (min, max) = match (self.min, self.max) {
                    (Some(min), Some(max)) => (min, max),
                    _ => {
                        return Err(ExtError::ManifestInvalid(format!(
                            "parameter '{}' of kind float needs min and max",
                            self.id
                        )))
                    }
                };
                if !(min < max) {
                    return Err(ExtError::ManifestInvalid(format!(
                        "parameter '{}' has min {min} and max {max}; min must be smaller",
                        self.id
                    )));
                }
                if self.choices.is_some() {
                    return Err(ExtError::ManifestInvalid(format!(
                        "parameter '{}' is a float and cannot have choices",
                        self.id
                    )));
                }
                ParameterKind::Float { min, max }
            }
            "bool" => {
                if self.min.is_some() || self.max.is_some() {
                    return Err(ExtError::ManifestInvalid(format!(
                        "parameter '{}' is a bool and cannot have min or max",
                        self.id
                    )));
                }
                if self.choices.is_some() {
                    return Err(ExtError::ManifestInvalid(format!(
                        "parameter '{}' is a bool and cannot have choices",
                        self.id
                    )));
                }
                ParameterKind::Bool
            }
            "choice" => {
                let choices = match &self.choices {
                    Some(choices) if !choices.is_empty() => choices.clone(),
                    _ => {
                        return Err(ExtError::ManifestInvalid(format!(
                            "parameter '{}' of kind choice needs a non-empty choices list",
                            self.id
                        )))
                    }
                };
                if self.min.is_some() || self.max.is_some() {
                    return Err(ExtError::ManifestInvalid(format!(
                        "parameter '{}' is a choice and cannot have min or max",
                        self.id
                    )));
                }
                for (position, choice) in choices.iter().enumerate() {
                    if choices[..position]
                        .iter()
                        .any(|earlier| earlier == choice)
                    {
                        return Err(ExtError::ManifestInvalid(format!(
                            "parameter '{}' lists the choice '{choice}' twice",
                            self.id
                        )));
                    }
                }
                ParameterKind::Choice { choices }
            }
            other => {
                return Err(ExtError::ManifestInvalid(format!(
                    "parameter '{}' has unknown kind '{other}'; \
                     the kinds are float, bool and choice",
                    self.id
                )))
            }
        };

        let default = match &self.default {
            Some(value) => default_from_value(&self.id, &kind, value)?,
            None => {
                return Err(ExtError::ManifestInvalid(format!(
                    "parameter '{}' needs a default",
                    self.id
                )))
            }
        };

        Ok(Parameter {
            id: self.id.clone(),
            name: self.name.clone(),
            index,
            kind,
            default,
            unit: self.unit.clone(),
            description: self.description.clone(),
        })
    }
}

fn default_from_value(
    id: &str,
    kind: &ParameterKind,
    value: &toml::Value,
) -> Result<f32, ExtError> {
    match kind {
        ParameterKind::Float { min, max } => {
            let number = match value {
                toml::Value::Float(number) => *number,
                toml::Value::Integer(number) => *number as f64,
                other => {
                    return Err(ExtError::ManifestInvalid(format!(
                        "default for '{id}' must be a number, not {other}"
                    )))
                }
            };
            if number < *min || number > *max {
                return Err(ExtError::ManifestInvalid(format!(
                    "default for '{id}' ({number}) is outside min..max ({min}..{max})"
                )));
            }
            Ok(number as f32)
        }
        ParameterKind::Bool => match value {
            toml::Value::Boolean(flag) => Ok(if *flag { 1.0 } else { 0.0 }),
            other => Err(ExtError::ManifestInvalid(format!(
                "default for '{id}' must be true or false, not {other}"
            ))),
        },
        ParameterKind::Choice { choices } => match value {
            toml::Value::Integer(index) => {
                let index = *index;
                if index < 0 || index as usize >= choices.len() {
                    return Err(ExtError::ManifestInvalid(format!(
                        "default for '{id}' is choice {index}, \
                         but there are {} choices",
                        choices.len()
                    )));
                }
                Ok(index as f32)
            }
            toml::Value::String(name) => {
                match choices
                    .iter()
                    .position(|choice| choice.eq_ignore_ascii_case(name))
                {
                    Some(index) => Ok(index as f32),
                    None => Err(ExtError::ManifestInvalid(format!(
                        "default for '{id}' is '{name}', \
                         which is not one of: {}",
                        choices.join(", ")
                    ))),
                }
            }
            other => Err(ExtError::ManifestInvalid(format!(
                "default for '{id}' must be a choice index or name, not {other}"
            ))),
        },
    }
}

fn validate_id(id: &str) -> Result<(), ExtError> {
    let allowed = |character: char| {
        character.is_ascii_alphanumeric() || character == '.' || character == '-' || character == '_'
    };
    if id.is_empty() || !id.contains('.') || !id.chars().all(allowed) {
        return Err(ExtError::ManifestInvalid(format!(
            "id '{id}' must be reverse-DNS, like com.example.my-effect"
        )));
    }
    if id.starts_with('.') || id.ends_with('.') || id.contains("..") {
        return Err(ExtError::ManifestInvalid(format!(
            "id '{id}' has an empty part; each part must name something"
        )));
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<(), ExtError> {
    // The numeric core has to be there; a pre-release or build suffix may follow.
    let core = version
        .split(['-', '+'])
        .next()
        .unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    let numeric = !parts.is_empty()
        && parts.len() <= 4
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    if !version.starts_with(|c: char| c.is_ascii_digit()) || !numeric {
        return Err(ExtError::ManifestInvalid(format!(
            "version '{version}' must be a semantic version, like 0.1.0"
        )));
    }
    Ok(())
}

fn validate_capabilities(capabilities: &[String]) -> Result<(), ExtError> {
    for capability in capabilities {
        if abi::CAPABILITIES_RESERVED.contains(&capability.as_str()) {
            return Err(ExtError::CapabilityDenied(format!(
                "'{capability}' is reserved and not supported in API version {}",
                abi::API_VERSION
            )));
        }
        if !abi::CAPABILITIES.contains(&capability.as_str()) {
            return Err(ExtError::CapabilityDenied(format!(
                "'{capability}' is not a known capability; \
                 API version {} has {}",
                abi::API_VERSION,
                abi::CAPABILITIES.join(", ")
            )));
        }
    }
    for (position, capability) in capabilities.iter().enumerate() {
        if capabilities[..position].iter().any(|earlier| earlier == capability) {
            return Err(ExtError::ManifestInvalid(format!(
                "capability '{capability}' is listed twice"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
api_version = 1
id = "com.example.gain-invert"
name = "Gain / Invert"
version = "0.1.0"
capabilities = ["audio.read", "audio.write"]

[[parameters]]
id = "gain_db"
name = "Gain (dB)"
kind = "float"
min = -60.0
max = 24.0
default = 0.0

[[parameters]]
id = "invert"
name = "Invert polarity"
kind = "bool"
default = false
"#;

    #[test]
    fn a_valid_manifest_parses_with_parameters_in_order() {
        let manifest = Manifest::from_str(MINIMAL).expect("valid");
        assert_eq!(manifest.api_version, 1);
        assert_eq!(manifest.id, "com.example.gain-invert");
        assert_eq!(manifest.parameters.len(), 2);
        assert_eq!(manifest.parameters[0].id, "gain_db");
        assert_eq!(manifest.parameters[0].index, 0);
        assert_eq!(manifest.parameters[1].id, "invert");
        assert_eq!(manifest.parameters[1].index, 1);
        assert_eq!(manifest.parameters[1].default, 0.0);
        assert!(manifest.can_process_audio());
    }

    #[test]
    fn a_bool_default_of_true_becomes_one() {
        let text = MINIMAL.replace("default = false", "default = true");
        let manifest = Manifest::from_str(&text).expect("valid");
        assert_eq!(manifest.parameters[1].default, 1.0);
    }

    #[test]
    fn a_choice_default_may_be_a_name() {
        let text = format!(
            "{MINIMAL}\n[[parameters]]\nid = \"mode\"\nname = \"Mode\"\nkind = \"choice\"\n\
             choices = [\"Add\", \"Replace\"]\ndefault = \"Replace\"\n"
        );
        let manifest = Manifest::from_str(&text).expect("valid");
        assert_eq!(manifest.parameters[2].default, 1.0);
    }

    #[test]
    fn the_module_and_manifest_files_are_where_the_api_says() {
        let dir = Path::new("/tmp/example");
        assert!(Manifest::path_in(dir).ends_with("extension.toml"));
        assert!(Manifest::module_path_in(dir).ends_with("extension.wasm"));
    }

    #[test]
    fn values_are_parsed_against_their_kind() {
        let manifest = Manifest::from_str(MINIMAL).expect("valid");
        let gain = manifest.parameter("gain_db").expect("gain_db");
        assert_eq!(gain.parse_value("-6.0").expect("number"), -6.0);
        assert!(matches!(
            gain.parse_value("60"),
            Err(ExtError::InvalidArgument(_))
        ));
        assert!(matches!(
            gain.parse_value("loud"),
            Err(ExtError::InvalidArgument(_))
        ));

        let invert = manifest.parameter("invert").expect("invert");
        assert_eq!(invert.parse_value("true").expect("bool"), 1.0);
        assert_eq!(invert.parse_value("off").expect("bool"), 0.0);
    }
}
