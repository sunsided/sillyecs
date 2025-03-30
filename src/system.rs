use crate::Name;
use crate::component::ComponentName;
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Deref;

#[derive(Debug, Serialize, Deserialize)]
pub struct System {
    pub name: SystemName,
    pub inputs: Vec<ComponentName>,
    pub outputs: Vec<ComponentName>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SystemName(Name);

impl Deref for SystemName {
    type Target = Name;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SystemName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let type_name = String::deserialize(deserializer)?;
        Ok(Self(Name::new(type_name, "System")))
    }
}
