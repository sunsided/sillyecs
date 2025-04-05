mod archetype;
mod code;
mod component;
mod ecs;
mod system;
mod system_scheduler;
mod world;
mod state;

pub use crate::code::EcsCode;
use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Eq, PartialOrd, Ord, Hash, Serialize)]
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

impl PartialEq for Name {
    fn eq(&self, other: &Self) -> bool {
        self.type_name.eq(&other.type_name)
    }
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

fn pluralize_name(field_name: String) -> String {
    let field_name = if let Some(prefix) = field_name.strip_suffix('y') {
        format!("{prefix}ies")
    } else if !field_name.ends_with('s') {
        format!("{field_name}s")
    } else {
        field_name
    };
    field_name
}

fn snake_case_filter(value: String) -> String {
    pascal_to_snake(&value.trim())
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
}
