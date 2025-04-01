use crate::ecs::{Ecs, EcsError};
use crate::snake_case_filter;
use minijinja::{Environment, context};
use std::fs::File;
use std::io::{BufReader, Write};
use std::{env, io};

#[derive(Default)]
pub struct EcsCode {
    pub components: String,
    pub archetypes: String,
    pub systems: String,
    pub world: String,
}

#[derive(thiserror::Error, Debug)]
pub enum WriteCodeError {
    #[error("Could not access directory {0}: {1}")]
    InvalidDirectory(String, io::Error),
    #[error("Failed to open file {0}: {1}")]
    FailedToOpenFile(String, io::Error),
    #[error("Failed to write to file {0}: {1}")]
    FailedToWriteFile(String, io::Error),
}

impl EcsCode {
    pub fn generate<R>(reader: BufReader<R>) -> Result<EcsCode, EcsError>
    where
        R: io::Read,
    {
        let mut ecs: Ecs = serde_yaml::from_reader(reader).expect("Failed to deserialize ecs.yaml");
        ecs.ensure_component_consistency()?;
        ecs.ensure_distinct_archetype_components()?;
        ecs.ensure_system_consistency()?;
        ecs.scheduled_systems()?;
        ecs.finish();

        if !ecs.systems.is_empty() {
            debug_assert_ne!(ecs.scheduled_systems.len(), 0, "Some systems should be scheduled");
        }

        let mut env = Environment::new();
        env.add_filter("snake_case", snake_case_filter);
        env.add_filter("length", length);

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
        Ok(EcsCode {
            components: component_code,
            archetypes: archetype_code,
            world: world_code,
            systems: system_code,
            ..EcsCode::default()
        })
    }

    /// Writes generated code to multiple files in the output directory specified
    /// by the `OUT_DIR` environment variable.
    ///
    /// # Returns
    /// - `Ok(())` if all files are written successfully.
    /// - `Err(WriteCodeError)` if the `OUT_DIR` environment variable is not set
    ///   or is not a valid directory, or if there is an error opening or writing
    ///   to any file.
    ///
    /// # Errors
    /// This function returns a `WriteCodeError` in the following cases:
    /// - If the `OUT_DIR` environment variable is not set or points to an invalid directory.
    /// - If a file cannot be created in the specified directory.
    /// - If a file fails to write the content.
    pub fn write_files(&self) -> Result<(), WriteCodeError> {
        let out_dir = env::var("OUT_DIR").map_err(|_| {
            WriteCodeError::InvalidDirectory(
                String::from("(OUT_DIR)"),
                io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "The specified path is not a directory",
                ),
            )
        })?;
        self.write_files_to(out_dir)
    }

    /// Writes generated code to multiple files in the specified output directory.
    ///
    /// # Parameters
    /// - `out_dir`: The output directory path where the files will be written.
    ///
    /// # Returns
    /// - `Ok(())` if all files are written successfully.
    /// - `Err(WriteCodeError)` if there is an error opening or writing to any file.
    ///
    /// # Files Written
    /// - `components.gen.rs`: Contains the generated code for components.
    /// - `archetypes.gen.rs`: Contains the generated code for archetypes.
    /// - `systems.gen.rs`: Contains the generated code for systems.
    /// - `world.gen.rs`: Contains the generated code for the world.
    ///
    /// # Errors
    /// This function returns a `WriteCodeError` in the following cases:
    /// - If a file cannot be created in the specified directory.
    /// - If a file fails to write the content.
    pub fn write_files_to<P>(&self, out_dir: P) -> Result<(), WriteCodeError>
    where
        P: AsRef<str>,
    {
        let out_dir = out_dir.as_ref();

        if !std::path::Path::new(out_dir).is_dir() {
            return Err(WriteCodeError::InvalidDirectory(
                out_dir.to_string(),
                io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "The specified path is not a directory",
                ),
            ));
        }

        Self::write_file(out_dir, "components.gen.rs", &self.components)?;
        Self::write_file(out_dir, "archetypes.gen.rs", &self.archetypes)?;
        Self::write_file(out_dir, "systems.gen.rs", &self.systems)?;
        Self::write_file(out_dir, "world.gen.rs", &self.world)?;
        Ok(())
    }

    fn write_file(out_dir: &str, file_name: &str, content: &str) -> Result<(), WriteCodeError> {
        let path = format!("{out_dir}/{file_name}");
        let mut file =
            File::create(path).map_err(|e| WriteCodeError::FailedToOpenFile(e.to_string(), e))?;
        file.write_all(content.as_bytes())
            .map_err(|e| WriteCodeError::FailedToWriteFile(e.to_string(), e))?;
        Ok(())
    }
}

use minijinja::Error;
use minijinja::value::{Value, ValueKind};

fn length(value: Value) -> Result<Value, Error> {
    match value.kind() {
        ValueKind::Undefined => todo!("undefined"),
        ValueKind::None => todo!("none"),
        ValueKind::Bool => todo!("bool"),
        ValueKind::Number => todo!("number"),
        ValueKind::String => todo!("string"),
        ValueKind::Bytes => todo!("bytes"),
        ValueKind::Seq => Ok(Value::from(value.len())),
        ValueKind::Map => todo!("map"),
        ValueKind::Iterable => todo!("iterable"),
        ValueKind::Plain => todo!("plain"),
        ValueKind::Invalid => todo!("invalid"),
        _ => todo!("unknown"),
    }

    /*
    match value.kind() {
        // ValueKind::Array => Ok(Value::from(value.as_array().unwrap().len())),
        ValueKind::Iterable => Ok(Value::from(value.as_map().unwrap().len()))
        ValueKind::Map => Ok(Value::from(value.as_map().unwrap().len())),
        ValueKind::String => Ok(Value::from(value.as_str().unwrap().chars().count())),
        _ => Err(Error::new(format!(
            "Expected an array, map, or string but got {:?}",
            value.kind()
        ))),
    }
     */
}
