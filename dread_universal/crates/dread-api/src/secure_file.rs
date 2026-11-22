//! Bounded, no-follow reads for trust-critical local files.
//!
//! On Unix, the final path component is opened with O_NOFOLLOW and O_NONBLOCK.
//! Type, size and caller-specific ownership/mode policy are then checked on
//! the opened descriptor, not on a path that can be swapped between checks.

use std::{fs, path::Path};

#[cfg(unix)]
use std::io::Read;

pub(crate) struct OpenedRegularFile {
    pub(crate) bytes: Vec<u8>,
    pub(crate) metadata: fs::Metadata,
}

pub(crate) fn read_regular_file_no_follow(
    path: &Path,
    max_bytes: u64,
    description: &str,
) -> Result<OpenedRegularFile, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        if max_bytes == 0 {
            return Err(format!("{description} size limit must be nonzero"));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let file = options.open(path).map_err(|error| {
            format!("cannot safely open {description} without following symlinks: {error}")
        })?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("cannot inspect opened {description}: {error}"))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
            return Err(format!(
                "{description} must be a regular nonempty file no larger than {max_bytes} bytes"
            ));
        }

        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read opened {description}: {error}"))?;
        if bytes.is_empty() || bytes.len() as u64 > max_bytes {
            return Err(format!("{description} is empty or exceeds its size limit"));
        }
        Ok(OpenedRegularFile { bytes, metadata })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, max_bytes, description);
        Err("trust-critical no-follow file reads are supported only on Unix hosts".into())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::symlink,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dread-secure-file-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn nofollow_reader_rejects_symlinks_and_accepts_regular_files() {
        let directory = temp_dir();
        let target = directory.join("target.json");
        fs::write(&target, b"{\"signed\":true}").unwrap();
        let opened = read_regular_file_no_follow(&target, 1024, "test evidence file").unwrap();
        assert_eq!(opened.bytes, b"{\"signed\":true}");
        assert!(opened.metadata.is_file());

        let link = directory.join("evidence.json");
        symlink(&target, &link).unwrap();
        assert!(read_regular_file_no_follow(&link, 1024, "test evidence file").is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn nofollow_reader_rejects_zero_limits_and_oversized_files() {
        let directory = temp_dir();
        let path = directory.join("evidence.json");
        fs::write(&path, b"12345").unwrap();
        assert!(read_regular_file_no_follow(&path, 0, "test evidence file").is_err());
        assert!(read_regular_file_no_follow(&path, 4, "test evidence file").is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
