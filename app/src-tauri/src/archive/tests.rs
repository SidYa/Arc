//! Round trips through every writable format, passwords, partial extraction and unsafe names.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("arc-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// docs/ with a Cyrillic name, a nested folder, an empty file and an empty folder
fn sample(root: &Path) -> PathBuf {
    let src = root.join("docs");
    fs::create_dir_all(src.join("вкладена/глибше")).unwrap();
    fs::create_dir_all(src.join("empty")).unwrap();
    fs::write(src.join("readme.txt"), "hello archive").unwrap();
    fs::write(src.join("вкладена/звіт.txt"), "звіт ".repeat(5000)).unwrap();
    fs::write(src.join("вкладена/глибше/data.bin"), (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect::<Vec<_>>()).unwrap();
    fs::write(src.join("zero.txt"), "").unwrap();
    src
}

fn options<'a>(dest: &'a Path, password: Option<&'static str>) -> ExtractOptions<'a> {
    ExtractOptions { dest, password, items: None, base: String::new(), conflict: Conflict::Rename, expected_files: None }
}

fn assert_same(src: &Path, out: &Path) {
    for item in walkdir::WalkDir::new(src) {
        let item = item.unwrap();
        let rel = item.path().strip_prefix(src.parent().unwrap()).unwrap();
        let other = out.join(rel);
        assert!(other.exists(), "missing {}", other.display());
        if item.file_type().is_file() {
            assert_eq!(fs::read(item.path()).unwrap(), fs::read(&other).unwrap(), "content of {}", rel.display());
        }
    }
}

fn round_trip(format: Format, password: Option<&'static str>) {
    let tmp = TempDir::new(&format!("{}-{}", format.extension(), password.is_some()));
    let src = sample(&tmp.0);
    let archive = tmp.0.join(format!("out.{}", format.extension()));
    let opts = CreateOptions { format, level: 5, password: password.map(String::from), encrypt_names: false };
    create(std::slice::from_ref(&src), &archive, &opts, &Progress::default()).unwrap();
    assert!(archive.exists());

    let listing = list(&archive, password, &Progress::default()).unwrap();
    assert_eq!(listing.encrypted, password.is_some());
    let names: HashSet<_> = listing.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(names.contains("docs/вкладена/звіт.txt"), "{names:?}");
    assert!(names.contains("docs/zero.txt"));

    if password.is_some() {
        let out = tmp.0.join("nopass");
        let err = extract(&archive, &options(&out, None), &Progress::default()).unwrap_err();
        assert!(matches!(err, Error::PasswordRequired), "{err}");
        let err = extract(&archive, &options(&out, Some("wrong")), &Progress::default()).unwrap_err();
        assert!(matches!(err, Error::WrongPassword), "{err}");
    }

    let out = tmp.0.join("out");
    let progress = Progress::default();
    extract(&archive, &options(&out, password), &progress).unwrap();
    assert_same(&src, &out);
    assert!(out.join("docs/empty").is_dir());
    let (done, total) = progress.snapshot();
    assert!(total > 0 && done >= total, "{done}/{total}");

    // Only one folder, taken from inside "docs"
    let part = tmp.0.join("part");
    let mut opts = options(&part, password);
    opts.items = Some(HashSet::from(["docs/вкладена".to_string()]));
    opts.base = "docs".into();
    extract(&archive, &opts, &Progress::default()).unwrap();
    assert!(part.join("вкладена/глибше/data.bin").exists());
    assert!(!part.join("readme.txt").exists());
    assert!(!part.join("docs").exists());

    // A second extraction keeps the existing files
    extract(&archive, &options(&out, password), &Progress::default()).unwrap();
    assert!(out.join("docs/readme (1).txt").exists());
}

#[test]
fn zip_round_trip() {
    round_trip(Format::Zip, None);
}

#[test]
fn zip_aes() {
    round_trip(Format::Zip, Some("пароль-123"));
}

#[test]
fn sevenz_round_trip() {
    round_trip(Format::SevenZ, None);
}

#[test]
fn sevenz_aes() {
    round_trip(Format::SevenZ, Some("пароль-123"));
}

#[test]
fn tar_gz_round_trip() {
    round_trip(Format::TarGz, None);
}

#[test]
fn tar_xz_round_trip() {
    round_trip(Format::TarXz, None);
}

#[test]
fn pause_holds_the_job() {
    let tmp = TempDir::new("pause");
    let src = sample(&tmp.0);
    let archive = tmp.0.join("p.7z");
    let progress = std::sync::Arc::new(Progress::default());
    progress.set_paused(true);
    let worker = {
        let (progress, archive) = (progress.clone(), archive.clone());
        std::thread::spawn(move || {
            let opts = CreateOptions { format: Format::SevenZ, level: 1, password: None, encrypt_names: false };
            create(&[src], &archive, &opts, &progress)
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(progress.snapshot().0, 0, "nothing is read while paused");
    assert!(!worker.is_finished());
    progress.set_paused(false);
    worker.join().unwrap().unwrap();
    assert!(archive.exists());
}

#[test]
fn sevenz_encrypted_names() {
    let tmp = TempDir::new("7z-names");
    let src = sample(&tmp.0);
    let archive = tmp.0.join("secret.7z");
    let opts = CreateOptions { format: Format::SevenZ, level: 1, password: Some("pw".into()), encrypt_names: true };
    create(&[src], &archive, &opts, &Progress::default()).unwrap();
    assert!(matches!(list(&archive, None, &Progress::default()), Err(Error::PasswordRequired)));
    assert!(matches!(list(&archive, Some("bad"), &Progress::default()), Err(Error::WrongPassword)));
    assert!(list(&archive, Some("pw"), &Progress::default()).is_ok());
}

#[test]
fn cancel_removes_partial_archive() {
    let tmp = TempDir::new("cancel");
    let src = sample(&tmp.0);
    let archive = tmp.0.join("c.zip");
    let progress = Progress::default();
    progress.cancel();
    let opts = CreateOptions { format: Format::Zip, level: 5, password: None, encrypt_names: false };
    assert!(matches!(create(&[src], &archive, &opts, &progress), Err(Error::Cancelled)));
    assert!(!archive.exists());
    assert!(!tmp.0.join("c.zip.part").exists());
}

#[test]
fn unsafe_names_stay_inside() {
    let dest = Path::new("C:/out");
    assert_eq!(safe_join(dest, "../evil.txt"), None);
    assert_eq!(safe_join(dest, "a/../../evil.txt"), None);
    assert_eq!(safe_join(dest, "/abs/x"), Some(dest.join("abs").join("x")));
    assert_eq!(safe_join(dest, "C:/Windows/x"), None);
    assert_eq!(safe_join(dest, "ok\\nested.txt"), Some(dest.join("ok").join("nested.txt")));
}

#[test]
fn oem_zip_names() {
    // "Звіт" in CP1125 (Ukrainian Windows) and "Звит" in CP866 (Russian)
    assert_eq!(zip::decode_oem(&[0x87, 0xA2, 0xF7, 0xE2], 1125), "Звіт");
    assert_eq!(zip::decode_oem(&[0x87, 0xA2, 0xA8, 0xE2], 866), "Звит");
    assert_eq!(zip::decode_oem(&[0x80, b'a'], 437), "Ça");

    // Explorer-style archive: no UTF-8 flag
    let tmp = TempDir::new("oem");
    let archive = tmp.0.join("old.zip");
    write_raw_zip(&archive, &[0x87, 0xA2, 0xA8, 0xE2, b'.', b't', b'x', b't'], b"data");
    let listing = list(&archive, None, &Progress::default()).unwrap();
    assert!(listing.entries[0].path.starts_with("Зв"), "{}", listing.entries[0].path);
    let out = tmp.0.join("out");
    extract(&archive, &options(&out, None), &Progress::default()).unwrap();
    assert_eq!(fs::read(out.join(&listing.entries[0].path)).unwrap(), b"data");
}

/// A stored ZIP with one file whose name bytes are given as is.
fn write_raw_zip(path: &Path, name: &[u8], data: &[u8]) {
    let crc = {
        let mut table = [0u32; 256];
        for i in 0..256u32 {
            let mut c = i;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            }
            table[i as usize] = c;
        }
        !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
    };
    let mut out = Vec::new();
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    le32(&mut out, 0x04034b50);
    for x in [20u16, 0, 0, 0, 0x21] {
        le16(&mut out, x);
    }
    le32(&mut out, crc);
    le32(&mut out, data.len() as u32);
    le32(&mut out, data.len() as u32);
    le16(&mut out, name.len() as u16);
    le16(&mut out, 0);
    out.extend_from_slice(name);
    out.extend_from_slice(data);
    let central = out.len() as u32;
    le32(&mut out, 0x02014b50);
    for x in [20u16, 20, 0, 0, 0, 0x21] {
        le16(&mut out, x);
    }
    le32(&mut out, crc);
    le32(&mut out, data.len() as u32);
    le32(&mut out, data.len() as u32);
    le16(&mut out, name.len() as u16);
    for x in [0u16, 0, 0, 0] {
        le16(&mut out, x);
    }
    le32(&mut out, 0);
    le32(&mut out, 0);
    out.extend_from_slice(name);
    let central_size = out.len() as u32 - central;
    le32(&mut out, 0x06054b50);
    for x in [0u16, 0, 1, 1] {
        le16(&mut out, x);
    }
    le32(&mut out, central_size);
    le32(&mut out, central);
    le16(&mut out, 0);
    fs::write(path, out).unwrap();
}

#[test]
fn stems() {
    assert_eq!(archive_stem(Path::new("a/photos.tar.gz")), "photos");
    assert_eq!(archive_stem(Path::new("movie.part01.rar")), "movie");
    assert_eq!(archive_stem(Path::new("v1.2.zip")), "v1.2");
}

// RAR fixtures from the unrar crate (MIT/Apache): Arc can't create RAR archives itself.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/archive/testdata").join(name)
}

#[test]
fn rar_list_and_extract() {
    for name in ["solid.rar", "unicode.rar"] {
        let listing = list(&fixture(name), None, &Progress::default()).unwrap();
        assert!(!listing.entries.is_empty(), "{name}");
        let tmp = TempDir::new(&format!("rar-{name}"));
        let progress = Progress::default();
        extract(&fixture(name), &options(&tmp.0, None), &progress).unwrap();
        for e in listing.entries.iter().filter(|e| !e.is_dir) {
            let file = tmp.0.join(&e.path);
            assert_eq!(fs::metadata(&file).unwrap().len(), e.size, "{}", file.display());
        }
        let (done, total) = progress.snapshot();
        assert_eq!(done, total);
    }
}

#[test]
fn rar_numeric_name_is_not_a_volume() {
    // UnRAR treats "movie.2024.rar" as multipart VOL1 "movie.0001.rar"; keep the name as is
    let tmp = TempDir::new("rar-num");
    let archive = tmp.0.join("RealBiomes V2.0 UE5.3.rar");
    fs::copy(fixture("unicode.rar"), &archive).unwrap();
    let listing = list(&archive, None, &Progress::default()).unwrap();
    assert!(!listing.entries.is_empty());
    extract(&archive, &options(&tmp.0, None), &Progress::default()).unwrap();
}

#[test]
fn rar_password() {
    // Encrypted data, readable names
    let path = fixture("crypted.rar");
    let listing = list(&path, None, &Progress::default()).unwrap();
    assert!(listing.encrypted);
    let tmp = TempDir::new("rar-pw");
    assert!(matches!(extract(&path, &options(&tmp.0, None), &Progress::default()), Err(Error::PasswordRequired)));
    let wrong = extract(&path, &options(&tmp.0, Some("nope")), &Progress::default()).unwrap_err();
    assert!(matches!(wrong, Error::WrongPassword), "{wrong}");
    extract(&path, &options(&tmp.0, Some("unrar")), &Progress::default()).unwrap();
    assert!(listing.entries.iter().filter(|e| !e.is_dir).all(|e| tmp.0.join(&e.path).exists()));

    // Encrypted names: nothing without the password
    let path = fixture("comment-hpw-password.rar");
    assert!(matches!(list(&path, None, &Progress::default()), Err(Error::PasswordRequired)));
    assert!(!list(&path, Some("password"), &Progress::default()).unwrap().entries.is_empty());
}

/// Many entries of different sizes through the parallel ZIP extraction: every one complete and in place.
#[test]
fn zip_many_files_parallel() {
    let tmp = TempDir::new("zip-many");
    let src = tmp.0.join("many");
    for i in 0..300u32 {
        let dir = src.join(format!("d{}", i % 7));
        fs::create_dir_all(&dir).unwrap();
        let data: Vec<u8> = (0..(i * 997 % 70_000)).map(|j| (j.wrapping_mul(i + 3) % 253) as u8).collect();
        fs::write(dir.join(format!("f{i}.bin")), data).unwrap();
    }
    let archive = tmp.0.join("many.zip");
    let opts = CreateOptions { format: Format::Zip, level: 1, password: None, encrypt_names: false };
    create(std::slice::from_ref(&src), &archive, &opts, &Progress::default()).unwrap();
    let out = tmp.0.join("out");
    let progress = Progress::default();
    extract(&archive, &options(&out, None), &progress).unwrap();
    assert_same(&src, &out);
    let (done, total) = progress.snapshot();
    assert_eq!(done, total);
}
/// Files are compressed by several threads, and still land in the archive in their order,
/// with their times, next to folders and to files too large for the threads' memory.
#[test]
fn zip_parallel_keeps_order_and_times() {
    let tmp = TempDir::new("zip-order");
    let src = tmp.0.join("data");
    fs::create_dir_all(src.join("sub")).unwrap();
    for i in 0..60 {
        let dir = if i % 3 == 0 { src.join("sub") } else { src.clone() };
        fs::write(dir.join(format!("f{i:02}.txt")), format!("file {i} ").repeat(i * 300 + 1)).unwrap();
    }
    let sources = collect(std::slice::from_ref(&src), &[]).unwrap();
    for password in [None, Some("пароль")] {
        let archive = tmp.0.join(format!("out-{}.zip", password.is_some()));
        let opts = CreateOptions { format: Format::Zip, level: 5, password: password.map(String::from), encrypt_names: false };
        create(std::slice::from_ref(&src), &archive, &opts, &Progress::default()).unwrap();
        let listing = list(&archive, password, &Progress::default()).unwrap();
        let names: Vec<_> = listing.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, sources.iter().map(|s| s.name.as_str()).collect::<Vec<_>>());
        for (entry, source) in listing.entries.iter().zip(&sources) {
            assert_eq!(entry.size, source.size, "{}", entry.path);
            assert_eq!(entry.encrypted, password.is_some() && !source.is_dir, "{}", entry.path);
            // DOS time: 2 seconds
            assert!((entry.modified.unwrap() - source.unix_time()).abs() <= 2, "{}", entry.path);
        }
        let out = tmp.0.join(format!("x-{}", password.is_some()));
        extract(&archive, &options(&out, password), &Progress::default()).unwrap();
        assert_same(&src, &out);
    }
}

/// What happens to a file that is already there, and to a file named like an existing folder.
#[test]
fn conflicts() {
    let tmp = TempDir::new("conflicts");
    let src = sample(&tmp.0);
    let archive = tmp.0.join("out.zip");
    let opts = CreateOptions { format: Format::Zip, level: 1, password: None, encrypt_names: false };
    create(std::slice::from_ref(&src), &archive, &opts, &Progress::default()).unwrap();
    let out = tmp.0.join("out");
    extract(&archive, &options(&out, None), &Progress::default()).unwrap();
    let readme = out.join("docs/readme.txt");

    fs::write(&readme, "mine").unwrap();
    let mut skip = options(&out, None);
    skip.conflict = Conflict::Skip;
    extract(&archive, &skip, &Progress::default()).unwrap();
    assert_eq!(fs::read_to_string(&readme).unwrap(), "mine");
    assert!(!out.join("docs/readme (1).txt").exists());

    let mut overwrite = options(&out, None);
    overwrite.conflict = Conflict::Overwrite;
    extract(&archive, &overwrite, &Progress::default()).unwrap();
    assert_eq!(fs::read_to_string(&readme).unwrap(), "hello archive");
    assert!(!out.join("docs/readme (1).txt").exists());

    // A folder has the file's name: the file goes next to it
    fs::remove_file(&readme).unwrap();
    fs::create_dir(&readme).unwrap();
    extract(&archive, &options(&out, None), &Progress::default()).unwrap();
    assert!(readme.is_dir());
    assert_eq!(fs::read_to_string(out.join("docs/readme.txt (1)")).unwrap(), "hello archive");
}

/// Files of several chunks (compressed by several threads at once), one that cannot be compressed,
/// one that is exactly a chunk long, an empty one.
fn big_sample(root: &Path) -> PathBuf {
    let src = root.join("big");
    fs::create_dir_all(src.join("empty-dir")).unwrap();
    let mut seed = 7u32;
    let mut noise = |len: usize, letters: bool| -> Vec<u8> {
        (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if letters { b"the quick brown fox \n"[(seed >> 24) as usize % 21] } else { (seed >> 24) as u8 }
            })
            .collect()
    };
    fs::write(src.join("text.txt"), noise(5_300_000, true)).unwrap();
    fs::write(src.join("random.bin"), noise(2_500_000, false)).unwrap();
    fs::write(src.join("chunk.txt"), noise(1 << 20, true)).unwrap();
    fs::write(src.join("small-random.bin"), noise(3000, false)).unwrap();
    fs::write(src.join("zero"), "").unwrap();
    src
}

#[test]
fn zip_big_files() {
    let tmp = TempDir::new("zip-big");
    let src = big_sample(&tmp.0);
    for (level, password) in [(5, None), (5, Some("secret-123")), (0, None), (0, Some("secret-123")), (9, None)] {
        let archive = tmp.0.join(format!("big-{level}-{}.zip", password.is_some()));
        let opts = CreateOptions { format: Format::Zip, level, password: password.map(String::from), encrypt_names: false };
        let progress = Progress::default();
        create(std::slice::from_ref(&src), &archive, &opts, &progress).unwrap();
        let (done, total) = progress.snapshot();
        assert_eq!(done, total);
        let size = fs::metadata(&archive).unwrap().len();
        if level > 0 {
            // The text packs well; what doesn't pack is not made larger
            assert!(size < 6_200_000, "{size}");
        }
        if password.is_some() {
            let out = tmp.0.join("wrong");
            let err = extract(&archive, &options(&out, Some("wrong")), &Progress::default()).unwrap_err();
            assert!(matches!(err, Error::WrongPassword), "{err}");
        }
        let out = tmp.0.join(format!("out-{level}-{}", password.is_some()));
        extract(&archive, &options(&out, password), &Progress::default()).unwrap();
        assert_same(&src, &out);
        if let Ok(keep) = std::env::var("ARC_KEEP") {
            fs::copy(&archive, Path::new(&keep).join(archive.file_name().unwrap())).unwrap();
        }
    }
}

/// The 64-bit fields, reached without 4 GB of data by lowering their limit.
#[test]
fn zip64_fields() {
    let tmp = TempDir::new("zip64");
    let src = big_sample(&tmp.0);
    let sources = collect(std::slice::from_ref(&src), &[]).unwrap();
    for password in [None, Some("секрет")] {
        let archive = tmp.0.join(format!("zip64-{}.zip", password.is_some()));
        let file = fs::File::create(&archive).unwrap();
        zipwrite::create_with(file, &sources, 5, password, &Progress::default(), 1000).unwrap();
        let listing = list(&archive, password, &Progress::default()).unwrap();
        assert_eq!(listing.entries.len(), sources.len());
        for (entry, source) in listing.entries.iter().zip(&sources) {
            assert_eq!((entry.path.as_str(), entry.size), (source.name.as_str(), source.size));
        }
        let out = tmp.0.join(format!("out-{}", password.is_some()));
        extract(&archive, &options(&out, password), &Progress::default()).unwrap();
        assert_same(&src, &out);
        if let Ok(keep) = std::env::var("ARC_KEEP") {
            fs::copy(&archive, Path::new(&keep).join(archive.file_name().unwrap())).unwrap();
        }
    }
}

/// gzip written by several threads is one ordinary stream.
#[test]
fn tar_gz_big_files() {
    let tmp = TempDir::new("tgz-big");
    let src = big_sample(&tmp.0);
    for level in [0, 1, 5, 9] {
        let archive = tmp.0.join(format!("big-{level}.tar.gz"));
        let opts = CreateOptions { format: Format::TarGz, level, password: None, encrypt_names: false };
        create(std::slice::from_ref(&src), &archive, &opts, &Progress::default()).unwrap();
        let out = tmp.0.join(format!("out-{level}"));
        extract(&archive, &options(&out, None), &Progress::default()).unwrap();
        assert_same(&src, &out);
        if let Ok(keep) = std::env::var("ARC_KEEP") {
            fs::copy(&archive, Path::new(&keep).join(archive.file_name().unwrap())).unwrap();
        }
    }
}
