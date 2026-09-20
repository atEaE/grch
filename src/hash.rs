use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, Result};
use md5::{Digest, Md5};
use sha1::Sha1;

const BUF_LEN: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHashes {
    pub size: u64,
    pub crc: u32,
    pub md5: [u8; 16],
    pub sha1: [u8; 20],
}

/// Compute size, crc32, md5 and sha1 in a single pass without loading the whole file.
pub fn hash_file(path: &Path) -> Result<FileHashes> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    hash_reader(file, BUF_LEN)
}

fn hash_reader(mut reader: impl Read, buf_len: usize) -> Result<FileHashes> {
    let mut buf = vec![0u8; buf_len];
    let mut size = 0u64;
    let mut crc = crc32fast::Hasher::new();
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        let chunk = &buf[..n];
        crc.update(chunk);
        md5.update(chunk);
        sha1.update(chunk);
        size += n as u64;
    }

    Ok(FileHashes {
        size,
        crc: crc.finalize(),
        md5: md5.finalize().into(),
        sha1: sha1.finalize().into(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn hash_file_known_vectors() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        let empty = temp_dir.path().join("empty");
        let abc = temp_dir.path().join("abc");
        fs::write(&empty, b"").unwrap();
        fs::write(&abc, b"abc").unwrap();

        // act
        let empty_hashes = hash_file(&empty).unwrap();
        let abc_hashes = hash_file(&abc).unwrap();

        // assert
        assert_eq!(
            empty_hashes,
            FileHashes {
                size: 0,
                crc: 0x00000000,
                md5: [
                    0xD4, 0x1D, 0x8C, 0xD9, 0x8F, 0x00, 0xB2, 0x04, 0xE9, 0x80, 0x09, 0x98, 0xEC,
                    0xF8, 0x42, 0x7E
                ],
                sha1: [
                    0xDA, 0x39, 0xA3, 0xEE, 0x5E, 0x6B, 0x4B, 0x0D, 0x32, 0x55, 0xBF, 0xEF, 0x95,
                    0x60, 0x18, 0x90, 0xAF, 0xD8, 0x07, 0x09
                ],
            }
        );
        assert_eq!(
            abc_hashes,
            FileHashes {
                size: 3,
                crc: 0x352441C2,
                md5: [
                    0x90, 0x01, 0x50, 0x98, 0x3C, 0xD2, 0x4F, 0xB0, 0xD6, 0x96, 0x3F, 0x7D, 0x28,
                    0xE1, 0x7F, 0x72
                ],
                sha1: [
                    0xA9, 0x99, 0x3E, 0x36, 0x47, 0x06, 0x81, 0x6A, 0xBA, 0x3E, 0x25, 0x71, 0x78,
                    0x50, 0xC2, 0x6C, 0x9C, 0xD0, 0xD8, 0x9D
                ],
            }
        );
    }

    #[test]
    fn hash_reader_is_independent_of_buffer_size() {
        // arrange
        let data: Vec<u8> = (0..(3 * 64 + 17)).map(|i| (i * 7 % 251) as u8).collect();
        let expected = FileHashes {
            size: data.len() as u64,
            crc: crc32fast::hash(&data),
            md5: Md5::digest(&data).into(),
            sha1: Sha1::digest(&data).into(),
        };

        // act & assert
        for buf_len in [1, 7, 64, 100, data.len(), data.len() + 1] {
            assert_eq!(
                hash_reader(data.as_slice(), buf_len).unwrap(),
                expected,
                "buf_len={buf_len}"
            );
        }
    }
}
