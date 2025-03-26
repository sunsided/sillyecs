use serde::Deserialize;
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

fn generate_component_code(ecs: &Ecs) -> String {
    let mut generated_code = String::new();
    for component in &ecs.components {
        generated_code.push_str(&format!("pub struct {};\n", component.name));
    }
    generated_code
}

fn generate_archetype_code(ecs: &Ecs) -> String {
    let mut generated_code = String::new();
    for archetype in &ecs.archetypes {
        generated_code.push_str(&format!("pub struct {} {{\n", archetype.name));
        for component in &archetype.components {
            generated_code.push_str(&format!(
                "    pub {}: {},\n",
                component.to_lowercase(),
                component
            ));
        }
        generated_code.push_str("}\n\n");
    }
    generated_code
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
