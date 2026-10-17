use std::{
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
};

use futures::future;
use reqwest::Client;
use zip::result::ZipError;

use crate::{
    deps::manifest::Manifest,
    error::Error,
    package_name::PackageName,
    paths::{self, CacheKey},
    telemetry::EventListener,
};

use super::manifest::Package;

pub struct Downloader<'a> {
    http: Client,
    root_path: &'a Path,
}

impl<'a> Downloader<'a> {
    pub fn new(root_path: &'a Path) -> Self {
        Self {
            http: Client::new(),
            root_path,
        }
    }

    pub async fn download_packages<I, T>(
        &self,
        event_listener: &T,
        packages: I,
        project_name: &PackageName,
        manifest: &mut Manifest,
    ) -> Result<Vec<(PackageName, bool)>, Error>
    where
        T: EventListener,
        I: Iterator<Item = &'a Package>,
    {
        let mut tasks = vec![];

        for package in packages.filter(|package| project_name != &package.name) {
            let cache_key =
                paths::CacheKey::new(&self.http, event_listener, package, manifest).await?;
            let task = self.ensure_package_in_build_directory(package, cache_key);
            tasks.push(task);
        }

        future::try_join_all(tasks).await
    }

    pub async fn ensure_package_in_build_directory(
        &self,
        package: &Package,
        cache_key: CacheKey,
    ) -> Result<(PackageName, bool), Error> {
        let downloaded = self
            .ensure_package_downloaded(package, &cache_key)
            .await
            .map(|downloaded| (package.name.clone(), downloaded))?;
        self.extract_package_from_cache(&package.name, &cache_key)
            .await?;
        Ok(downloaded)
    }

    pub async fn ensure_package_downloaded(
        &self,
        package: &Package,
        cache_key: &CacheKey,
    ) -> Result<bool, Error> {
        let packages_cache_path = paths::packages_cache();

        let zipball_path = paths::package_cache_zipball(cache_key);

        if !packages_cache_path.exists() {
            tokio::fs::create_dir_all(packages_cache_path).await?;
        }

        if zipball_path.is_file() {
            return Ok(false);
        }

        let url = format!(
            "https://api.github.com/repos/{}/{}/zipball/{}",
            package.name.owner, package.name.repo, package.version
        );

        let response = self
            .http
            .get(url)
            .header("User-Agent", "aiken-lang")
            .send()
            .await?;

        if response.status().as_u16() >= 400 {
            return Err(Error::UnknownPackageVersion {
                package: Box::new(package.clone()),
            });
        }

        let bytes = response.bytes().await?;

        tokio::fs::write(&zipball_path, bytes).await?;

        Ok(true)
    }

    pub async fn extract_package_from_cache(
        &self,
        name: &PackageName,
        cache_key: &CacheKey,
    ) -> Result<(), Error> {
        let destination = self.root_path.join(paths::build_deps_package(name));

        tokio::fs::create_dir_all(&destination).await?;

        let zipball_path = self.root_path.join(paths::package_cache_zipball(cache_key));

        let zipball = tokio::fs::read(zipball_path).await?;

        tokio::task::spawn_blocking(move || install_package(&zipball, &destination)).await??;

        Ok(())
    }
}

// Extract in a private, empty directory so existing package symlinks cannot
// redirect writes. A malformed archive leaves the last valid package intact.
fn install_package(bytes: &[u8], destination: &Path) -> Result<(), ZipError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("Missing package parent"))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".aiken-package-")
        .tempdir_in(parent)?;
    extract_zip(&mut archive, staging.path())?;
    match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(destination)?,
        Ok(_) => std::fs::remove_file(destination)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    std::fs::rename(staging.path(), destination)?;
    Ok(())
}

fn invalid(message: &'static str) -> ZipError {
    ZipError::InvalidArchive(message.into())
}

fn extract_zip<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    directory: &Path,
) -> Result<(), ZipError> {
    use std::{ffi::OsString, fs, path::Component};
    let mut root: Option<OsString> = None;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name()?;
        if name.starts_with(['/', '\\'])
            || name.as_bytes().get(1) == Some(&b':')
            || name.split(['/', '\\']).any(|component| component == "..")
        {
            return Err(invalid(
                "Absolute and parent paths are forbidden in package archives",
            ));
        }
        if file.is_symlink()
            || file
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
        {
            return Err(invalid(
                "Package archives may contain only regular files and directories",
            ));
        }
        let enclosed = file
            .enclosed_name()
            .ok_or_else(|| invalid("Invalid file path"))?;
        let mut components = enclosed.components();
        let Some(Component::Normal(top)) = components.next() else {
            return Err(invalid("Missing package root directory"));
        };
        if root.as_ref().is_some_and(|root| root != top) {
            return Err(invalid("Multiple package root directories"));
        }
        root = Some(top.to_owned());
        let filepath: PathBuf = components.collect();
        // enclosed_name validates the whole path. Validate again after removing
        // the GitHub root: root/../outside would otherwise escape this directory.
        if filepath
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(invalid("Invalid path after removing package root"));
        }
        if filepath.as_os_str().is_empty() {
            if file.is_dir() {
                continue;
            }
            return Err(invalid("Missing package root directory"));
        }
        let outpath = directory.join(filepath);
        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&outpath)?;
            std::io::copy(&mut file, &mut output)?;
            #[cfg(unix)]
            if let Some(mode) = file.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                // Never preserve SUID, SGID, sticky, or archive file-type bits.
                output.set_permissions(fs::Permissions::from_mode(mode & 0o777))?;
            }
        }
    }
    if root.is_none() {
        return Err(invalid("Empty package archive"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    fn archive(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for &(name, content, mode) in files {
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .external_attributes(mode << 16);
            writer.start_file(name, options).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn extracts_github_package_without_preserving_special_permission_bits() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("package");
        let bytes = archive(&[("repo-sha/lib/main.ak", b"pub fn main() { True }", 0o107755)]);
        install_package(&bytes, &destination).unwrap();
        let path = destination.join("lib/main.ak");
        assert_eq!(std::fs::read(&path).unwrap(), b"pub fn main() { True }");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o7777,
                0o755
            );
        }
    }

    #[test]
    fn rejects_traversal_absolute_paths_and_multiple_roots_without_replacing_valid_package() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("package");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("original"), b"keep").unwrap();
        for path in [
            "repo/../outside",
            "../outside",
            "/repo/outside",
            "C:/repo/outside",
            "repo/../../outside",
            "repo\\..\\outside",
        ] {
            let bytes = archive(&[(path, b"bad", 0o100644)]);
            assert!(
                install_package(&bytes, &destination).is_err(),
                "accepted {path}"
            );
            assert_eq!(
                std::fs::read(destination.join("original")).unwrap(),
                b"keep"
            );
            assert!(!root.path().join("outside").exists());
        }
        let bytes = archive(&[
            ("one/file", b"one", 0o100644),
            ("two/file", b"two", 0o100644),
        ]);
        assert!(install_package(&bytes, &destination).is_err());
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            1,
            "failed staging directory leaked"
        );
    }

    #[test]
    fn rejects_symlink_entries_duplicate_files_and_corrupt_archives() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("package");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink("repo/link", "../../outside", SimpleFileOptions::default())
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(install_package(&bytes, &destination).is_err());
        // The ZIP format permits names that normalize to the same output path.
        let bytes = archive(&[
            ("repo/lib/main.ak", b"first", 0o100644),
            ("repo/lib/./main.ak", b"second", 0o100644),
        ]);
        assert!(install_package(&bytes, &destination).is_err());
        assert!(install_package(b"not a zip archive", &destination).is_err());
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn existing_package_symlinks_cannot_redirect_extraction() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        let destination = root.path().join("package");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("main.ak"), b"keep").unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::os::unix::fs::symlink(&outside, destination.join("lib")).unwrap();
        let bytes = archive(&[("repo/lib/main.ak", b"new", 0o100644)]);
        install_package(&bytes, &destination).unwrap();
        assert_eq!(std::fs::read(outside.join("main.ak")).unwrap(), b"keep");
        assert_eq!(
            std::fs::read(destination.join("lib/main.ak")).unwrap(),
            b"new"
        );
        assert!(
            !std::fs::symlink_metadata(destination.join("lib"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
