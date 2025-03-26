use serde::Deserialize;
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
    pub name: String,
    pub components: Vec<ComponentRef>,
}

#[derive(Debug, Deserialize)]
pub struct Component {
    pub name: String,
}

pub type ComponentRef = String;

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

    println!("{}", component_code);
    println!("{}", archetype_code);
    Ok(Code {
        components: component_code,
        archetypes: archetype_code,
        ..Code::default()
    })
}

fn ensure_distinct_archetype_components(ecs: &Ecs) -> Result<(), EcsError> {
    let mut archetype_component_sets: HashMap<String, String> = HashMap::new();
    for archetype in &ecs.archetypes {
        let mut component_set = archetype.components.iter().cloned().collect::<Vec<_>>();
        component_set.sort_unstable();
        let component_set = component_set.join("+").to_ascii_lowercase();
        if let Some(duplicate) = archetype_component_sets.get(&component_set) {
            return Err(EcsError::DuplicateArchetype(
                archetype.name.clone(),
                duplicate.clone(),
            ));
        }
        archetype_component_sets.insert(component_set.clone(), archetype.name.clone());
    }
    Ok(())
}

fn generate_component_code(ecs: &Ecs) -> String {
    let mut generated_code = String::new();

    generated_code.push_str("/// The ID of a [`Component`].\n#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]\npub struct ComponentId(u64);\n\n");

    generated_code.push_str(&format!(
        "/// Marker trait for components.\npub trait Component: 'static + Send + Sync {{\n    const ID: ComponentId;\n}}\n\n",
    ));

    for (id, component) in ecs.components.iter().enumerate() {
        generated_code.push_str(&format!(
            "#[derive(Debug)]\npub struct {name}Component({name}Data);\n",
            name = component.name
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl Component for {name}Component {{\n    const ID: ComponentId = ComponentId({id});\n}}\n",
            name = component.name
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl From<{name}Data> for {name}Component {{\n    fn from(data: {name}Data) -> Self {{\n        Self(data)\n    }}\n}}\n\n",
            name = component.name
        ));

        generated_code.push_str(&format!(
            "\n#[automatically_derived]\nimpl std::ops::Deref for {name}Component {{\n    type Target = {name}Data;\n\n    fn deref(&self) -> &Self::Target {{\n        &self.0\n    }}\n}}\n\n",
            name = component.name
        ));

        generated_code.push_str(&format!(
            "#[automatically_derived]\nimpl std::ops::DerefMut for {name}Component {{\n    fn deref_mut(&mut self) -> &mut Self::Target {{\n        &mut self.0\n    }}\n}}\n\n",
            name = component.name
        ));
    }
    generated_code
}

fn generate_archetype_code(ecs: &Ecs) -> String {
    let mut generated_code = String::new();
    for archetype in &ecs.archetypes {
        generated_code.push_str(&format!("pub struct {} {{\n", archetype.name));
        for component in &archetype.components {
            let field_name = pascal_to_snake(component);
            generated_code.push_str(&format!("    pub {field_name}: {component}Component,\n",));
        }
        generated_code.push_str("}\n\n");
    }
    generated_code
}

fn pascal_to_snake(component: &ComponentRef) -> String {
    let field_name = component
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
                    component_ref.clone(),
                    archetype.name.clone(),
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
            ("HTTPServer", "http_server"),
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
