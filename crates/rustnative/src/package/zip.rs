//! A reproducible ZIP writer.
//!
//! Two builds of the same files produce byte-identical archives: entries
//! are sorted by path, every timestamp is the same fixed value, and files
//! are stored rather than compressed — so nothing depends on a compressor's
//! version or on the order the file system happened to list a folder in.
//! A portable build that differs between machines cannot be checked against
//! a hash, and a hash is the only thing a person downloading a `.zip` has.
//!
//! Stored rather than deflated is a deliberate trade: the archive is bigger
//! than it could be, and it is reproducible without depending on a
//! compression library's exact output. An application that wants a smaller
//! download has an MSIX.

use std::path::Path;

/// One file in the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path inside the archive, with `/` separators.
    pub name: String,
    /// Its contents.
    pub data: Vec<u8>,
}

/// The MS-DOS timestamp every entry carries: 1980-01-01 00:00, the
/// earliest the format can express.
const FIXED_TIME: u16 = 0;
const FIXED_DATE: u16 = 0x0021;

/// Builds a ZIP archive holding `entries`, sorted by name.
#[must_use]
pub fn archive(mut entries: Vec<Entry>) -> Vec<u8> {
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries.dedup_by(|left, right| left.name == right.name);

    let mut out = Vec::new();
    let mut directory = Vec::new();
    for entry in &entries {
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let crc = crc32fast::hash(&entry.data);
        let size = u32::try_from(entry.data.len()).unwrap_or(u32::MAX);
        let name = entry.name.as_bytes();

        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&FIXED_TIME.to_le_bytes());
        out.extend_from_slice(&FIXED_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&u16::try_from(name.len()).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // no extra field
        out.extend_from_slice(name);
        out.extend_from_slice(&entry.data);

        // Central directory record, built as we go and appended below.
        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&20u16.to_le_bytes()); // version made by
        directory.extend_from_slice(&20u16.to_le_bytes()); // version needed
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&FIXED_TIME.to_le_bytes());
        directory.extend_from_slice(&FIXED_DATE.to_le_bytes());
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&u16::try_from(name.len()).unwrap_or(u16::MAX).to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes()); // extra
        directory.extend_from_slice(&0u16.to_le_bytes()); // comment
        directory.extend_from_slice(&0u16.to_le_bytes()); // disk number
        directory.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        // Fixed external attributes: an ordinary read-write file, so the
        // archive does not carry this machine's umask or read-only bits.
        directory.extend_from_slice(&0x0000_0020u32.to_le_bytes());
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name);
    }

    let directory_offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let directory_size = u32::try_from(directory.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&directory);
    let count = u16::try_from(entries.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // directory's disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&directory_size.to_le_bytes());
    out.extend_from_slice(&directory_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // no comment
    out
}

/// The `SHA256SUMS` file for `entries`, in the format `sha256sum -c`
/// checks: the hash, two spaces, the name.
#[must_use]
pub fn checksums(entries: &[Entry]) -> String {
    let mut lines = entries
        .iter()
        .map(|entry| format!("{}  {}\n", hash_of(&entry.data), entry.name))
        .collect::<Vec<_>>();
    lines.sort();
    lines.concat()
}

/// The SHA-256 of `data`, as lower-case hexadecimal.
#[must_use]
pub fn hash_of(data: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};

    use std::fmt::Write as _;

    Sha256::digest(data).iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// Reads `path` into an entry named `name`.
///
/// # Errors
///
/// The file could not be read.
pub fn entry_from(path: &Path, name: &str) -> std::io::Result<Entry> {
    Ok(Entry { name: name.to_owned(), data: std::fs::read(path)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<Entry> {
        vec![
            Entry { name: "b.txt".to_owned(), data: b"second".to_vec() },
            Entry { name: "a.txt".to_owned(), data: b"first".to_vec() },
        ]
    }

    #[test]
    fn two_archives_of_the_same_files_are_byte_identical() {
        assert_eq!(archive(entries()), archive(entries()));
        // ...including when the files arrive in a different order.
        let mut reordered = entries();
        reordered.reverse();
        assert_eq!(archive(entries()), archive(reordered));
    }

    #[test]
    fn the_archive_is_a_zip_with_its_entries_sorted() {
        let zip = archive(entries());
        assert_eq!(&zip[..4], b"PK\x03\x04", "a local file header first");
        let first_name = &zip[30..35];
        assert_eq!(first_name, b"a.txt", "sorted, not as given");
        assert_eq!(&zip[zip.len() - 22..zip.len() - 18], b"PK\x05\x06", "and an end record last");
        let count = u16::from_le_bytes([zip[zip.len() - 14], zip[zip.len() - 13]]);
        assert_eq!(count, 2);
    }

    #[test]
    fn the_stored_data_is_the_file_itself_with_its_crc() {
        let zip = archive(vec![Entry { name: "x".to_owned(), data: b"hello".to_vec() }]);
        let crc = u32::from_le_bytes([zip[14], zip[15], zip[16], zip[17]]);
        assert_eq!(crc, crc32fast::hash(b"hello"));
        assert_eq!(&zip[31..36], b"hello");
    }

    #[test]
    fn checksums_are_sha256_in_the_format_sha256sum_checks() {
        // The SHA-256 of an empty input, which every implementation agrees
        // on, so this pins the hash itself rather than only its shape.
        assert_eq!(
            hash_of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let sums = checksums(&entries());
        assert_eq!(sums.lines().count(), 2);
        assert!(sums.contains("  a.txt"), "{sums}");
        assert!(sums.ends_with('\n'));
    }
}
