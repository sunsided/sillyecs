//! Build-time dependency of `sillyecs`, a silly little Archetype ECS system.

mod archetype;
mod code;
mod component;
mod ecs;
mod state;
mod system;
mod system_scheduler;
mod world;

pub use crate::code::EcsCode;
use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Name {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(rename = "raw")]
    pub type_name_raw: String,
    #[serde(rename = "field")]
    pub field_name: String,
    #[serde(rename = "fields")]
    pub field_name_plural: String,
}

impl Name {
    pub fn new(type_name: String, type_suffix: &str) -> Self {
        let field_name = pascal_to_snake(&type_name);
        let field_name_plural = pluralize_name(field_name.clone());
        let adjusted_type_name = if type_name.ends_with(type_suffix) {
            type_name.clone()
        } else {
            format!("{type_name}{type_suffix}")
        };
        Self {
            type_name: adjusted_type_name,
            type_name_raw: type_name,
            field_name,
            field_name_plural,
        }
    }
}

impl Display for Name {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.type_name)
    }
}

fn pluralize_name<S>(field_name: S) -> String
where
    S: AsRef<str>,
{
    // TODO: Implement proper handling of irregulars (mouse -> mice)

    let field_name = field_name.as_ref();

    if field_name.ends_with('y') {
        if field_name.len() >= 2 {
            let before_y = field_name.chars().nth_back(1).unwrap();
            if !"aeiou".contains(before_y) {
                return format!("{}ies", &field_name[..field_name.len() - 1]);
            }
        }
        return format!("{field_name}s");
    }

    let suffixes = ["ch", "sh", "x", "z"];
    if suffixes.iter().any(|s| field_name.ends_with(s)) {
        return format!("{field_name}es");
    }

    // Handle singular words ending in "s" carefully.
    // If it's "ss" (e.g., "boss"), treat as needing "es".
    // But if it ends in "s" and is not "ss", assume it's already plural.
    if field_name.ends_with("ss") {
        return format!("{field_name}es");
    } else if field_name.ends_with('s') {
        return field_name.to_string(); // likely already plural
    }

    format!("{field_name}s")
}

fn snake_case_filter(value: String) -> String {
    pascal_to_snake(value.trim())
}

fn pascal_to_snake(type_name: &str) -> String {
    let field_name = type_name
        .chars()
        .flat_map(|c| {
            if c.is_uppercase() {
                vec!['_', c.to_ascii_lowercase()]
            } else {
                vec![c]
            }
        })
        .skip_while(|&c| c == '_')
        .collect::<String>();
    field_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pascal_to_snake() {
        let cases = vec![
            ("PascalCase", "pascal_case"),
            ("SnakeCase", "snake_case"),
            ("HTTPServer", "h_t_t_p_server"),
            ("", ""),
            ("lowercase", "lowercase"),
            ("UPPERCASE", "u_p_p_e_r_c_a_s_e"),
            ("Mixed123Case", "mixed123_case"),
        ];

        for (input, expected) in cases {
            assert_eq!(pascal_to_snake(&input.to_string()), expected);
        }
    }

    #[test]
    fn test_pluralize_name() {
        assert_eq!(pluralize_name("velocity"), "velocities");
        assert_eq!(pluralize_name("component"), "components");
        assert_eq!(pluralize_name("TimeOfDay"), "TimeOfDays");
        assert_eq!(pluralize_name("box"), "boxes");
        assert_eq!(pluralize_name("brush"), "brushes");
        assert_eq!(pluralize_name("boss"), "bosses");
        assert_eq!(pluralize_name("fox"), "foxes");
        assert_eq!(pluralize_name("door"), "doors");
        assert_eq!(pluralize_name("stars"), "stars");
    }
}
