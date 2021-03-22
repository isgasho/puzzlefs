use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

extern crate serde_json;

use crate::descriptor::Descriptor;

// the OCI spec says this must be 2 in order for older dockers to use image layouts, and that it
// will probably be removed. We could hard code it to two, but let's use -1 as an additional
// indicator that this is a "weird" image. ...why is this defined as an int and not a uint? :)
const PUZZLEFS_SCHEMA_VERSION: i32 = -1;

// the name of the index file as defined by the OCI spec
pub const PATH: &str = "index.json";

#[derive(Serialize, Deserialize, Debug)]
pub struct Index {
    #[serde(rename = "schemaVersion")]
    version: i32,
    pub manifests: Vec<Descriptor>,
    pub annotations: HashMap<String, String>,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            version: PUZZLEFS_SCHEMA_VERSION,
            manifests: Vec::new(),
            annotations: HashMap::new(),
        }
    }
}

impl Index {
    pub(crate) fn open(p: &Path) -> Result<Index, Box<dyn std::error::Error>> {
        let index_file = fs::File::open(p)?;
        let index = serde_json::from_reader::<_, Index>(index_file)?;
        if index.version != PUZZLEFS_SCHEMA_VERSION {
            Err(Box::new(io::Error::new(
                io::ErrorKind::Other,
                format!("bad schema version {}", index.version),
            )))
        } else {
            Ok(index)
        }
    }

    pub(crate) fn write(&self, p: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let index_file = fs::File::create(p)?;
        serde_json::to_writer(index_file, &self)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_can_open_new_index() {
        let dir = tempdir().unwrap();
        let i = Index::default();
        i.write(&dir.path().join(PATH)).unwrap();
        Index::open(&dir.path().join(PATH)).unwrap();
    }
}
