use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::io;
use std::io::BufReader;

#[derive(Debug, Deserialize)]
pub struct Ecs {
    pub components: Vec<Component>,
    pub archetypes: Vec<Archetype>,
}

#[derive(Debug, Deserialize)]
pub struct Archetype {
    pub name: Name,
    pub components: Vec<ComponentRef>,
}

#[derive(Debug, Deserialize)]
pub struct Component {
    pub name: Name,
}

pub type ComponentRef = Name;

#[derive(thiserror::Error, Debug)]
pub enum EcsError {
    #[error("Component '{0}' in archetype '{1}' is not defined in the ECS components.")]
    MissingComponent(String, String),
    #[error("Duplicate archetype '{0}' and '{1}'")]
    DuplicateArchetype(String, String),
}

#[derive(Default)]
pub struct Code {
    pub components: String,
    pub archetypes: String,
    pub world: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name {
    pub type_name: String,
    pub field_name: String,
    pub field_name_plural: String,
}

impl<'de> Deserialize<'de> for Name {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        let field_name = pascal_to_snake(&type_name);
        let field_name_plural = pluralize_name(field_name.clone());
        Ok(Name {
            type_name,
            field_name,
            field_name_plural,
        })
    }
}

pub fn build<R>(reader: BufReader<R>) -> Result<Code, EcsError>
where
    R: io::Read,
{
    let ecs: Ecs = serde_yaml::from_reader(reader).expect("Failed to deserialize ecs.yaml");
    ensure_component_consistency(&ecs)?;
    ensure_distinct_archetype_components(&ecs)?;

    let component_code = generate_component_code(&ecs);
    let archetype_code = generate_archetype_code(&ecs);
    let world_code = generate_world(&ecs);

    println!("{}", component_code);
    println!("{}", archetype_code);
    Ok(Code {
        components: component_code,
        archetypes: archetype_code,
        world: world_code,
        ..Code::default()
    })
}

fn ensure_distinct_archetype_components(ecs: &Ecs) -> Result<(), EcsError> {
    let mut archetype_component_sets: HashMap<String, String> = HashMap::new();
    for archetype in &ecs.archetypes {
        let mut component_set = archetype
            .components
            .iter()
            .map(|c| c.type_name.clone())
            .collect::<Vec<_>>();
        component_set.sort_unstable();
        let component_set = component_set.join("+").to_ascii_lowercase();
        if let Some(duplicate) = archetype_component_sets.get(&component_set) {
            return Err(EcsError::DuplicateArchetype(
                archetype.name.type_name.clone(),
                duplicate.clone(),
            ));
        }
        archetype_component_sets.insert(component_set.clone(), archetype.name.type_name.clone());
    }
    Ok(())
}

fn generate_world(ecs: &Ecs) -> String {
    let mut generated_code = String::new();

    generated_code.push_str(
        "/// A world holding all archetypes.\n#[derive(Debug, Clone)]\npub struct World {\n",
    );
    for archetype in &ecs.archetypes {
        generated_code.push_str(&format!(
            "    pub {field_name}: {name},\n",
            field_name = archetype.name.field_name,
            name = archetype.name.type_name
        ));
    }
    generated_code.push_str("}\n\n");
    generated_code
}

fn generate_component_code(ecs: &Ecs) -> String {
    let mut generated_code = String::new();

    generated_code.push_str("/// The ID of a [`Component`].\n#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]\npub struct ComponentId(u64);\n\n");

    generated_code.push_str(&format!(
        "/// Marker trait for components.\npub trait Component: 'static + Send + Sync {{\n    const ID: ComponentId;\n}}\n\n",
    ));

    for (id, component) in ecs.components.iter().enumerate() {
        generated_code.push_str(&format!(
            "#[derive(Debug, Clone)]\npub struct {name}Component({name}Data);\n",
            name = component.name.type_name
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl Component for {name}Component {{\n    const ID: ComponentId = ComponentId({id});\n}}\n",
            name = component.name.type_name
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl From<{name}Data> for {name}Component {{\n    fn from(data: {name}Data) -> Self {{\n        Self(data)\n    }}\n}}\n\n",
            name = component.name.type_name
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl std::ops::Deref for {name}Component {{\n    type Target = {name}Data;\n\n    fn deref(&self) -> &Self::Target {{\n        &self.0\n    }}\n}}\n\n",
            name = component.name.type_name
        ));

        generated_code.push_str(&format!(
            "#[automatically_derived]\nimpl std::ops::DerefMut for {name}Component {{\n    fn deref_mut(&mut self) -> &mut Self::Target {{\n        &mut self.0\n    }}\n}}\n\n",
            name = component.name.type_name
        ));
    }
    generated_code
}

fn generate_archetype_code(ecs: &Ecs) -> String {
    let mut generated_code = String::new();
    for archetype in &ecs.archetypes {
        generated_code.push_str(
            "/// An archetype grouping entities with identical components.\n#[derive(Debug, Clone)]\n",
        );
        generated_code.push_str(&format!("pub struct {} {{\n", archetype.name.type_name));
        generated_code.push_str("    pub entities: Vec<EntityId>,\n");
        for component in &archetype.components {
            generated_code.push_str(&format!(
                "    pub {field_names}: Vec<{type_name}Component>,\n",
                field_names = component.field_name_plural,
                type_name = component.type_name
            ));
        }
        generated_code.push_str("}\n\n");
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

/// Ensure that all components used by archetypes are defined in the components vector of the ECS.
fn ensure_component_consistency(ecs: &Ecs) -> Result<(), EcsError> {
    let defined_components: std::collections::HashSet<_> = ecs
        .components
        .iter()
        .map(|component| &component.name)
        .collect();

    for archetype in &ecs.archetypes {
        for component_ref in &archetype.components {
            if !defined_components.contains(component_ref) {
                return Err(EcsError::MissingComponent(
                    component_ref.type_name.clone(),
                    archetype.name.type_name.clone(),
                ));
            }
        }
    }
    Ok(())
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
