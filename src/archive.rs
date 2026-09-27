use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sevenz_rust2::encoder_options::{AesEncoderOptions, Lzma2Options};
use sevenz_rust2::{ArchiveEntry, ArchiveReader, ArchiveWriter, Error as SevenzError};

use crate::hash;

const LZMA2_LEVEL: u32 = 6;
// Uncompressed bytes per independently compressed LZMA2 chunk. Larger chunks compress
// better, smaller chunks parallelize better; 32 MiB keeps a PSP ISO (~1 GB) at ~30 chunks.
const LZMA2_CHUNK_SIZE: u64 = 32 * 1024 * 1024;
const COPY_BUF_LEN: usize = 1 << 20;

/// Result of packing: the archive's sha256 (its remote object name) and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub sha256: String,
    pub size: u64,
}

/// What an extracted file must match before it is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub name: String,
    pub size: u64,
    pub crc32: u32,
}

/// Pack a single file into a 7z at `dest`: LZMA2 + AES-256 with header encryption, so
/// neither the content nor the entry name is readable without the password.
pub fn pack_file(src: &Path, entry_name: &str, dest: &Path, password: &str) -> Result<Packed> {
    let reader = File::open(src).with_context(|| format!("open {}", src.display()))?;
    let out = File::create(dest).with_context(|| format!("create {}", dest.display()))?;
    write_archive(out, entry_name, reader, password)?;

    let sha256 = hash::sha256_file(dest)?;
    let size = fs::metadata(dest)?.len();
    Ok(Packed { sha256, size })
}

/// Extract the single entry of `archive` into `dest_dir`, named after the entry.
/// With `expected`, the entry name, size and crc32 are verified first. The data is
/// written to a temp file in `tmp_dir` and renamed into place only after it verifies,
/// so a wrong password or a truncated download never leaves a bad file in `dest_dir`.
/// `tmp_dir` must be on the same filesystem as `dest_dir` for the rename to be atomic.
pub fn extract_file(
    archive: &Path,
    password: &str,
    expected: Option<&Expected>,
    dest_dir: &Path,
    tmp_dir: &Path,
) -> Result<PathBuf> {
    let reader = open_reader(
        File::open(archive).with_context(|| format!("open {}", archive.display()))?,
        password,
    )?;

    fs::create_dir_all(dest_dir)?;
    fs::create_dir_all(tmp_dir)?;
    let tmp = tempfile::NamedTempFile::new_in(tmp_dir)
        .with_context(|| format!("create temp file in {}", tmp_dir.display()))?;
    let mut sink = Verifier::new(tmp.as_file());
    let name = read_single_entry(reader, &mut sink)?;
    let (size, crc32) = sink.finish();

    if let Some(expected) = expected
        && (name != expected.name || size != expected.size || crc32 != expected.crc32)
    {
        bail!(
            "{}: extracted data does not match (name {:?}, size {} / crc32 {:08X}; expected size {} / {:08X})",
            expected.name,
            name,
            size,
            crc32,
            expected.size,
            expected.crc32
        );
    }
    let dest = dest_dir.join(safe_entry_name(&name)?);
    tmp.persist(&dest)
        .with_context(|| format!("place {}", dest.display()))?;
    Ok(dest)
}

fn write_archive<W: Write + Seek>(
    out: W,
    entry_name: &str,
    data: impl Read,
    password: &str,
) -> Result<()> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    let mut writer = ArchiveWriter::new(out).map_err(map_sevenz)?;
    writer.set_content_methods(vec![
        AesEncoderOptions::new(password.into()).into(),
        Lzma2Options::from_level_mt(LZMA2_LEVEL, threads, LZMA2_CHUNK_SIZE).into(),
    ]);
    writer.set_encrypt_header(true);
    writer
        .push_archive_entry(ArchiveEntry::new_file(entry_name), Some(data))
        .map_err(map_sevenz)?;
    writer.finish()?;
    Ok(())
}

fn open_reader<R: Read + Seek>(source: R, password: &str) -> Result<ArchiveReader<R>> {
    ArchiveReader::new(source, password.into()).map_err(map_sevenz)
}

/// Copy the one file entry to `sink` and return its name. Archives made by grch hold
/// exactly one entry; anything else is treated as corruption.
fn read_single_entry<R: Read + Seek>(
    mut reader: ArchiveReader<R>,
    mut sink: impl Write,
) -> Result<String> {
    let mut name = None;
    let mut extra = false;
    let mut io_failure = None;
    reader
        .for_each_entries(|entry, data| {
            if entry.is_directory() {
                return Ok(true);
            }
            if name.is_some() {
                extra = true;
                return Ok(false);
            }
            name = Some(entry.name().to_string());
            if let Err(e) = copy(data, &mut sink) {
                io_failure = Some(e);
                return Ok(false);
            }
            Ok(true)
        })
        .map_err(map_sevenz)?;
    if extra {
        bail!("archive holds more than one file");
    }
    if let Some(e) = io_failure {
        return Err(e).context("write extracted data");
    }
    name.ok_or_else(|| anyhow::anyhow!("archive holds no file"))
}

fn copy(reader: &mut dyn Read, writer: &mut impl Write) -> io::Result<()> {
    let mut buf = vec![0u8; COPY_BUF_LEN];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
    }
}

/// Entry names come from the archive, so never let them escape `dir`.
fn safe_entry_name(name: &str) -> Result<&str> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        bail!("unsafe entry name in archive: {:?}", name);
    }
    Ok(name)
}

/// The 7z header is encrypted, so a wrong password shows up as a header parse failure
/// rather than a clean "bad password" error. Both are reported as a password problem.
fn map_sevenz(e: SevenzError) -> anyhow::Error {
    match e {
        SevenzError::PasswordRequired
        | SevenzError::MaybeBadPassword(_)
        | SevenzError::NextHeaderCrcMismatch
        | SevenzError::ChecksumVerificationFailed => {
            anyhow::anyhow!("cannot decrypt archive (wrong password?): {}", e)
        }
        e => anyhow::anyhow!(e),
    }
}

/// Counts bytes and crc32 while passing data through to the inner writer.
struct Verifier<W: Write> {
    inner: W,
    size: u64,
    crc: crc32fast::Hasher,
}

impl<W: Write> Verifier<W> {
    fn new(inner: W) -> Self {
        Verifier {
            inner,
            size: 0,
            crc: crc32fast::Hasher::new(),
        }
    }

    fn finish(self) -> (u64, u32) {
        (self.size, self.crc.finalize())
    }
}

impl<W: Write> Write for Verifier<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.crc.update(&buf[..n]);
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const PASSWORD: &str = "correct horse battery staple";
    const NAME: &str = "Xxx (Japan).sfc";

    fn sample() -> Vec<u8> {
        (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn expected_for(data: &[u8]) -> Expected {
        Expected {
            name: NAME.to_string(),
            size: data.len() as u64,
            crc32: crc32fast::hash(data),
        }
    }

    /// Returns (archive path, original data). `dest`, `tmp` and `restored` are free dirs.
    fn packed_sample(temp: &TempDir) -> (PathBuf, Vec<u8>) {
        let data = sample();
        let src = temp.path().join(NAME);
        let dest = temp.path().join("out.7z");
        fs::write(&src, &data).unwrap();
        pack_file(&src, NAME, &dest, PASSWORD).unwrap();
        (dest, data)
    }

    #[test]
    fn pack_and_extract_roundtrip() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (archive, data) = packed_sample(&temp);
        let restored = temp.path().join("restored");
        let tmp = temp.path().join("tmp");

        // act
        let dest = extract_file(
            &archive,
            PASSWORD,
            Some(&expected_for(&data)),
            &restored,
            &tmp,
        )
        .unwrap();

        // assert
        assert_eq!(dest, restored.join(NAME));
        assert_eq!(fs::read(&dest).unwrap(), data);
        assert_eq!(fs::read_dir(&tmp).unwrap().count(), 0);
    }

    #[test]
    fn extract_without_expectation_uses_entry_name() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (archive, data) = packed_sample(&temp);
        let restored = temp.path().join("restored");

        // act
        let dest = extract_file(&archive, PASSWORD, None, &restored, &restored).unwrap();

        // assert
        assert_eq!(dest, restored.join(NAME));
        assert_eq!(fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn packed_reports_sha256_and_size_of_archive() {
        // arrange
        let temp = TempDir::new().unwrap();
        let data = sample();
        let src = temp.path().join(NAME);
        let dest = temp.path().join("out.7z");
        fs::write(&src, &data).unwrap();

        // act
        let packed = pack_file(&src, NAME, &dest, PASSWORD).unwrap();

        // assert
        assert_eq!(packed.sha256, hash::sha256_file(&dest).unwrap());
        assert_eq!(packed.size, fs::metadata(&dest).unwrap().len());
        assert_eq!(packed.sha256.len(), 64);
    }

    #[test]
    fn entry_name_is_not_visible_in_archive_bytes() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (archive, _data) = packed_sample(&temp);
        let bytes = fs::read(&archive).unwrap();
        // 7z stores names as UTF-16LE; check the UTF-8 form too in case that ever changes.
        let utf16: Vec<u8> = NAME.encode_utf16().flat_map(u16::to_le_bytes).collect();

        // act & assert
        assert!(!bytes.windows(utf16.len()).any(|w| w == utf16.as_slice()));
        assert!(!bytes.windows(NAME.len()).any(|w| w == NAME.as_bytes()));
    }

    #[test]
    fn no_password_cannot_list_entries() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (archive, _data) = packed_sample(&temp);

        // act & assert
        assert!(sevenz_rust2::Archive::open(&archive).is_err());
    }

    #[test]
    fn wrong_password_is_reported_without_writing_dest() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (archive, data) = packed_sample(&temp);
        let restored = temp.path().join("restored");
        let tmp = temp.path().join("tmp");

        // act
        let err = extract_file(
            &archive,
            "nope",
            Some(&expected_for(&data)),
            &restored,
            &tmp,
        )
        .unwrap_err();

        // assert
        assert!(err.to_string().contains("wrong password"), "{err:#}");
        assert!(!restored.join(NAME).exists());
        // The header fails to decrypt before anything is written, so tmp may not even exist.
        assert!(!tmp.exists() || fs::read_dir(&tmp).unwrap().count() == 0);
    }

    #[test]
    fn mismatched_expectation_is_rejected_without_writing_dest() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (archive, data) = packed_sample(&temp);
        let restored = temp.path().join("restored");
        let tmp = temp.path().join("tmp");
        let mut expected = expected_for(&data);
        expected.crc32 ^= 1;

        // act
        let err = extract_file(&archive, PASSWORD, Some(&expected), &restored, &tmp).unwrap_err();

        // assert
        assert!(err.to_string().contains("does not match"), "{err:#}");
        assert!(!restored.join(NAME).exists());
        assert_eq!(
            fs::read_dir(&tmp).unwrap().count(),
            0,
            "temp file was not cleaned up"
        );
    }

    #[test]
    fn mismatched_name_is_rejected() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (archive, data) = packed_sample(&temp);
        let restored = temp.path().join("restored");
        let mut expected = expected_for(&data);
        expected.name = "Other.sfc".to_string();

        // act
        let err =
            extract_file(&archive, PASSWORD, Some(&expected), &restored, &restored).unwrap_err();

        // assert
        assert!(err.to_string().contains("does not match"), "{err:#}");
        assert!(!restored.join(NAME).exists());
    }

    #[test]
    fn safe_entry_name_rejects_paths() {
        // act & assert
        assert!(safe_entry_name("a.sfc").is_ok());
        for bad in ["", ".", "..", "a/b", "a\\b", "../x"] {
            assert!(safe_entry_name(bad).is_err(), "{bad:?}");
        }
    }
}
