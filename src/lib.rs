mod archetype;
mod code;
mod component;
mod ecs;
mod system;

use crate::code::Code;
use crate::ecs::{Ecs, EcsError};
use minijinja::{Environment, context};
use serde::Serialize;
use std::io;
use std::io::BufReader;

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
        Self {
            type_name: format!("{type_name}{type_suffix}"),
            type_name_raw: type_name,
            field_name,
            field_name_plural,
        }
    }
}

pub fn build<R>(reader: BufReader<R>) -> Result<Code, EcsError>
where
    R: io::Read,
{
    let ecs: Ecs = serde_yaml::from_reader(reader).expect("Failed to deserialize ecs.yaml");
    ecs.ensure_component_consistency()?;
    ecs.ensure_distinct_archetype_components()?;

    let mut env = Environment::new();
    env.add_filter("snake_case", snake_case_filter);

    env.add_template("world", include_str!("../templates/world.rs.jinja2"))?;
    env.add_template(
        "components",
        include_str!("../templates/components.rs.jinja2"),
    )?;
    env.add_template(
        "archetypes",
        include_str!("../templates/archetypes.rs.jinja2"),
    )?;
    env.add_template("systems", include_str!("../templates/systems.rs.jinja2"))?;

    let world_code = env.get_template("world")?.render(context! {
        ecs => ecs,
    })?;

    let component_code = env.get_template("components")?.render(context! {
        ecs => ecs,
    })?;

    let archetype_code = env.get_template("archetypes")?.render(context! {
        ecs => ecs,
    })?;

    let system_code = env.get_template("systems")?.render(context! {
        ecs => ecs,
    })?;

    println!("{}", component_code);
    println!("{}", archetype_code);
    Ok(Code {
        components: component_code,
        archetypes: archetype_code,
        world: world_code,
        systems: system_code,
        ..Code::default()
    })
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
