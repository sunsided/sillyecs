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

    let world_code = env.get_template("world")?.render(context! {
        ecs => ecs,
    })?;

    let component_code = env.get_template("components")?.render(context! {
        ecs => ecs,
    })?;

    let archetype_code = env.get_template("archetypes")?.render(context! {
        ecs => ecs,
    })?;

    let system_code = generate_system_code(&ecs);

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

fn generate_system_code(ecs: &Ecs) -> String {
    let mut generated_code = String::new();

    generated_code.push_str("/// The ID of a [`System`].\n#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]\npub struct SystemId(u64);\n\n");

    generated_code.push_str(&format!(
        "/// Marker trait for systems.\npub trait System: 'static + Send + Sync {{\n    const ID: SystemId;\n}}\n\n",
    ));

    for (id, system) in ecs.systems.iter().enumerate() {
        generated_code.push_str(&format!(
            "#[derive(Debug, Clone)]\npub struct {name}({name}Data);\n",
            name = system.name.type_name,
        ));

        // TODO: How can we require that the system has this trait while not implementing it ourselves?
        generated_code.push_str(&format!(
            "\npub trait Apply{name} {{\n",
            name = system.name.type_name,
        ));
        generated_code.push_str("    /// Apply the system to the given entity components\n");
        // TODO Generate documentation comments for the components
        generated_code.push_str("    fn apply(&self");
        for input in &system.inputs {
            generated_code.push_str(&format!(
                ", {field_name}: &{type_name}",
                field_name = input.field_name,
                type_name = input.type_name
            ));
        }
        for output in &system.outputs {
            generated_code.push_str(&format!(
                ", {field_name}: &mut {type_name}",
                field_name = output.field_name,
                type_name = output.type_name
            ));
        }
        generated_code.push_str(");\n");
        generated_code.push_str("}\n");

        // TODO: Enforce the trait implementation.
        generated_code.push_str(&format!("\nimpl {name} {{\n", name = system.name.type_name,));
        generated_code.push_str("    /// Apply the system to the given entity components\n");
        generated_code.push_str("    #[inline]\n");
        generated_code.push_str("    pub fn apply(&self"); // TODO Add a function signature that takes vecs of components directly?
        for input in &system.inputs {
            generated_code.push_str(&format!(
                ", {field_name}: &{type_name}",
                field_name = input.field_name,
                type_name = input.type_name
            ));
        }
        for output in &system.outputs {
            generated_code.push_str(&format!(
                ", {field_name}: &mut {type_name}",
                field_name = output.field_name,
                type_name = output.type_name
            ));
        }
        generated_code.push_str(") {\n");
        generated_code.push_str(&format!(
            "        // Enforce the implementation of the system trait\n        Apply{name}::apply(self",
            name = system.name.type_name
        ));
        for input in &system.inputs {
            generated_code.push_str(&format!(", {field_name}", field_name = input.field_name));
        }
        for output in &system.outputs {
            generated_code.push_str(&format!(", {field_name}", field_name = output.field_name));
        }
        generated_code.push_str(");\n    }\n");
        generated_code.push_str("}\n");

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl System for {name} {{\n    const ID: SystemId = SystemId({id});\n}}\n",
            name = system.name.type_name
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl From<{name}Data> for {name} {{\n    fn from(data: {name}Data) -> Self {{\n        Self(data)\n    }}\n}}\n",
            name = system.name.type_name,
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl std::ops::Deref for {name} {{\n    type Target = {name}Data;\n\n    fn deref(&self) -> &Self::Target {{\n        &self.0\n    }}\n}}\n\n",
            name = system.name.type_name,
        ));

        generated_code.push_str(&format!(
            "#[automatically_derived]\nimpl std::ops::DerefMut for {name} {{\n    fn deref_mut(&mut self) -> &mut Self::Target {{\n        &mut self.0\n    }}\n}}\n\n",
            name = system.name.type_name
        ));
    }
    generated_code
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
