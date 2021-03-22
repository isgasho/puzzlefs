extern crate hex;

use std::convert::TryFrom;
use std::fs;
use std::io;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tee::TeeReader;
use tempfile::NamedTempFile;

use compression::{Compression, Decompressor};
use format::MetadataBlob;

mod descriptor;
pub use descriptor::Descriptor;

mod index;
pub use index::Index;

// this is a string, probably intended to be a real version format (though the spec doesn't say
// anything) so let's just say "puzzlefs-dev" for now since the format is in flux.
const PUZZLEFS_IMAGE_LAYOUT_VERSION: &str = "puzzlefs-dev";

const IMAGE_LAYOUT_PATH: &str = "oci-layout";

#[derive(Serialize, Deserialize, Debug)]
struct OCILayout {
    #[serde(rename = "imageLayoutVersion")]
    version: String,
}

pub struct Image<'a> {
    oci_dir: &'a Path,
}

impl<'a> Image<'a> {
    pub fn new(oci_dir: &'a Path) -> Result<Self, Box<dyn std::error::Error>> {
        let image = Image { oci_dir };
        fs::create_dir_all(image.blob_path())?;
        let layout_file = fs::File::create(oci_dir.join(IMAGE_LAYOUT_PATH))?;
        let layout = OCILayout {
            version: PUZZLEFS_IMAGE_LAYOUT_VERSION.to_string(),
        };
        serde_json::to_writer(layout_file, &layout)?;
        Ok(Image { oci_dir })
    }

    pub fn open(oci_dir: &'a Path) -> Result<Self, Box<dyn std::error::Error>> {
        let layout_file = fs::File::open(oci_dir.join(IMAGE_LAYOUT_PATH))?;
        let layout = serde_json::from_reader::<_, OCILayout>(layout_file)?;
        if layout.version != PUZZLEFS_IMAGE_LAYOUT_VERSION {
            Err(Box::new(io::Error::new(
                io::ErrorKind::Other,
                format!("bad image layout version {}", layout.version),
            )))
        } else {
            Ok(Image { oci_dir })
        }
    }

    pub fn blob_path(&self) -> PathBuf {
        self.oci_dir.join("blobs/sha256")
    }

    pub fn put_blob<R: io::Read, C: Compression>(&self, buf: R) -> Result<Descriptor, io::Error> {
        let tmp = NamedTempFile::new_in(self.oci_dir)?;
        let mut compressed = C::compress(tmp.reopen()?);
        let mut hasher = Sha256::new();

        let mut t = TeeReader::new(buf, &mut hasher);
        let size = io::copy(&mut t, &mut compressed)?;

        let digest = hasher.finalize();
        let descriptor = Descriptor::new(digest.into(), size);

        tmp.persist(self.blob_path().join(descriptor.digest_as_str()))?;
        Ok(descriptor)
    }

    fn open_raw_blob(&self, digest: &[u8; 32]) -> io::Result<fs::File> {
        fs::File::open(self.blob_path().join(hex::encode(digest)))
    }

    pub fn open_compressed_blob<C: Compression>(
        &self,
        digest: &[u8; 32],
    ) -> io::Result<Box<dyn Decompressor>> {
        let f = self.open_raw_blob(&digest)?;
        Ok(C::decompress(f))
    }

    pub fn open_metadata_blob<C: Compression>(
        &self,
        digest: &[u8; 32],
    ) -> io::Result<format::MetadataBlob> {
        let f = self.open_raw_blob(&digest)?;
        Ok(MetadataBlob::new::<C>(f))
    }

    pub fn fill_from_chunk(
        &self,
        chunk: format::BlobRef,
        addl_offset: u64,
        buf: &mut [u8],
    ) -> format::Result<usize> {
        let digest = &<[u8; 32]>::try_from(chunk)?;
        let mut blob = self.open_raw_blob(digest)?;
        blob.seek(io::SeekFrom::Start(chunk.offset + addl_offset))?;
        let n = blob.read(buf)?;
        Ok(n)
    }

    pub fn get_index(&self) -> Result<Index, Box<dyn std::error::Error>> {
        Index::open(&self.oci_dir.join(index::PATH))
    }

    pub fn put_index(&self, i: &Index) -> Result<(), Box<dyn std::error::Error>> {
        i.write(&self.oci_dir.join(index::PATH))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_put_blob_correct_hash() {
        let dir = tempdir().unwrap();
        let image: Image = Image::new(dir.path()).unwrap();
        let desc = image
            .put_blob::<_, compression::Noop>("meshuggah rocks".as_bytes())
            .unwrap();

        const DIGEST: &str = "3abd5ce0f91f640d88dca1f26b37037b02415927cacec9626d87668a715ec12d";
        assert_eq!(desc.digest_as_str(), DIGEST);

        let md = fs::symlink_metadata(image.blob_path().join(DIGEST)).unwrap();
        assert!(md.is_file());
    }

    #[test]
    fn test_open_can_open_new_image() {
        let dir = tempdir().unwrap();
        Image::new(dir.path()).unwrap();
        Image::open(dir.path()).unwrap();
    }

    #[test]
    fn test_put_get_index() {
        let dir = tempdir().unwrap();
        let image = Image::new(dir.path()).unwrap();
        let mut desc = image
            .put_blob::<_, compression::Noop>("meshuggah rocks".as_bytes())
            .unwrap();
        desc.set_name("foo".to_string());
        let mut index = Index::default();
        // TODO: make a real API for this that checks that descriptor has a name?
        index.manifests.push(desc);
        image.put_index(&index).unwrap();

        let image2 = Image::open(dir.path()).unwrap();
        let index2 = image2.get_index().unwrap();
        assert_eq!(index.manifests, index2.manifests);
    }
}
