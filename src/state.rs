use crate::Name;
use crate::system::{System, SystemNameRef};
use serde::{Deserialize, Deserializer, Serialize};
use std::hash::Hash;
use std::ops::Deref;

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pub name: StateName,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(skip_deserializing)]
    pub systems: Vec<SystemNameRef>,
}

impl State {
    pub(crate) fn finish(&mut self, systems: &[System]) {
        for system in systems {
            if system.states.iter().any(|s| s.state.eq(&self.name)) {
                self.systems.push(system.name.clone());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct StateName(pub(crate) Name);

pub type StateNameRef = StateName;

impl Deref for StateName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for StateName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "")))
    }
}
