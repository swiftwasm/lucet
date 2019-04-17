use crate::{
    traps::TrapManifestRecord,
    Error,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeMetadata {
    pub trap_manifest: Vec<TrapManifestRecord>,
}

impl CodeMetadata {
    pub fn new(
        trap_manifest: Vec<TrapManifestRecord>,
    ) -> Self {
        Self {
            trap_manifest,
        }
    }

    /// Serialize to [`bincode`](https://github.com/TyOverby/bincode).
    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        bincode::serialize(self).map_err(Error::SerializationError)
    }

    /// Deserialize from [`bincode`](https://github.com/TyOverby/bincode).
    pub fn deserialize(buf: &[u8]) -> Result<CodeMetadata, Error> {
        bincode::deserialize(buf).map_err(Error::DeserializationError)
    }
}
