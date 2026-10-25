use crate::ecs::EcsError;
use serde::{Deserialize, Serialize};

/// The default map type for the world's entity locations.
const DEFAULT_ENTITY_LOCATION_MAP: &str = "::std::collections::HashMap";

/// Where the generated code finds the user-defined types.
///
/// Each module path is optional. Without one, the generated code refers to the types by their
/// bare names, so they must be in scope where the code is included. With one, it uses fully
/// qualified paths and doesn't depend on its include site. After [`TypePaths::finish`], the
/// module paths are rendered as prefixes, i.e. either empty or ending in `::`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypePaths {
    /// The module holding the component data types (`<Name>Data`).
    #[serde(default)]
    pub components: String,
    /// The module holding the state types (`<Name>State`).
    #[serde(default)]
    pub states: String,
    /// The module holding the system data types (`<Name>SystemData`) of stateful systems.
    #[serde(default)]
    pub systems: String,
    /// The map type for the world's entity locations, taking a key and a value type parameter.
    #[serde(default = "default_entity_location_map")]
    pub entity_location_map: String,
}

impl Default for TypePaths {
    fn default() -> Self {
        Self {
            components: String::new(),
            states: String::new(),
            systems: String::new(),
            entity_location_map: default_entity_location_map(),
        }
    }
}

fn default_entity_location_map() -> String {
    DEFAULT_ENTITY_LOCATION_MAP.to_string()
}

impl TypePaths {
    /// Validates the paths and turns the module paths into prefixes for the templates.
    pub(crate) fn finish(&mut self) -> Result<(), EcsError> {
        for (key, path) in [
            ("components", &mut self.components),
            ("states", &mut self.states),
            ("systems", &mut self.systems),
        ] {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                path.clear();
                continue;
            }
            ensure_valid_path(key, trimmed)?;
            *path = format!("{trimmed}::");
        }

        let map = self.entity_location_map.trim().to_string();
        ensure_valid_path("entity_location_map", &map)?;
        self.entity_location_map = map;
        Ok(())
    }
}

/// Accepts `a::b::c`, optionally with a leading `::`, where each segment is an identifier.
fn ensure_valid_path(key: &'static str, path: &str) -> Result<(), EcsError> {
    let relative = path.strip_prefix("::").unwrap_or(path);
    let valid = !relative.is_empty()
        && relative.split("::").all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if valid {
        Ok(())
    } else {
        Err(EcsError::InvalidTypePath {
            key,
            path: path.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_paths_become_prefixes() {
        let mut paths = TypePaths {
            components: " crate::game::components ".to_string(),
            ..TypePaths::default()
        };
        paths.finish().expect("valid paths");
        assert_eq!(paths.components, "crate::game::components::");
        assert_eq!(paths.states, "");
        assert_eq!(paths.entity_location_map, DEFAULT_ENTITY_LOCATION_MAP);
    }

    #[test]
    fn invalid_paths_are_rejected() {
        for path in [
            "crate::",
            "crate::game:components",
            "crate::1st",
            "a b",
            "::",
        ] {
            let mut paths = TypePaths {
                states: path.to_string(),
                ..TypePaths::default()
            };
            assert!(
                matches!(
                    paths.finish(),
                    Err(EcsError::InvalidTypePath { key: "states", .. })
                ),
                "{path:?} must be rejected"
            );
        }
    }
}
