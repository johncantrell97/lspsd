fn main() {
    download::start().unwrap();
}

mod download {
    use anyhow::Context;
    use flate2::read::GzDecoder;
    use std::fs::File;
    use std::io::{self, BufReader, Cursor, Read};
    use std::path::Path;
    use tar::Archive;

    include!("src/versions.rs");

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn download_filename() -> String {
        format!("lspsd_{}_aarch64-apple-darwin.tar.gz", &VERSION)
    }

    // #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    // fn download_filename() -> String {
    //     format!("lspsd_{}_x86_64-linux-gnu.tar.gz", &VERSION)
    // }

    // #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    // fn download_filename() -> String {
    //     format!("lspsd_{}_aarch64-linux-gnu.tar.gz", &VERSION)
    // }

    // #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    // fn download_filename() -> String {
    //     format!("lspsd_{}_win64.zip", &VERSION)
    // }

    pub(crate) fn start() -> anyhow::Result<()> {
        if std::env::var_os("LSPSD_SKIP_DOWNLOAD").is_some() {
            return Ok(());
        }
        let download_filename = download_filename();
        let out_dir = std::env::var_os("OUT_DIR").unwrap();

        let mut lspsd_exe_home = Path::new(&out_dir).join("lspsd");
        if !lspsd_exe_home.exists() {
            std::fs::create_dir(&lspsd_exe_home)
                .with_context(|| format!("cannot create dir {:?}", lspsd_exe_home))?;
        }
        let existing_filename = lspsd_exe_home.join("lspsd");

        if !existing_filename.exists() {
            let (_file_or_url, tarball_bytes) = match std::env::var("LSPSD_TARBALL_FILE") {
                Err(_) => {
                    let download_endpoint = std::env::var("LSPSD_DOWNLOAD_ENDPOINT").unwrap_or(
                        "https://github.com/johncantrell97/lspsd/releases/download".to_owned(),
                    );

                    let url = format!("{}/{}/{}", download_endpoint, VERSION, download_filename);

                    let resp = minreq::get(&url)
                        .send()
                        .with_context(|| format!("cannot reach url {}", url))?;
                    assert_eq!(resp.status_code, 200, "url {} didn't return 200", url);

                    (url, resp.as_bytes().to_vec())
                }
                Ok(path) => {
                    let f = File::open(&path).with_context(|| {
                        format!(
                            "Cannot find {:?} specified with env var LSPSD_TARBALL_FILE",
                            &path
                        )
                    })?;
                    let mut reader = BufReader::new(f);
                    let mut buffer = Vec::new();
                    reader.read_to_end(&mut buffer)?;
                    (path, buffer)
                }
            };

            if download_filename.ends_with(".tar.gz") {
                let d = GzDecoder::new(&tarball_bytes[..]);
                let mut archive = Archive::new(d);
                
                for mut entry in archive.entries().unwrap().flatten() {
                    if let Ok(file) = entry.path() {
                      println!("cargo:warning=extracted file: {:?}", file);
                        if file.ends_with("lspsd") {
                            entry.unpack_in(&lspsd_exe_home).unwrap();
                        }
                    }
                }
            } else if download_filename.ends_with(".zip") {
                let cursor = Cursor::new(tarball_bytes);
                let mut archive = zip::ZipArchive::new(cursor).unwrap();
                for i in 0..zip::ZipArchive::len(&archive) {
                    let mut file = archive.by_index(i).unwrap();
                    let outpath = match file.enclosed_name() {
                        Some(path) => path.to_owned(),
                        None => continue,
                    };

                    if outpath.file_name().map(|s| s.to_str()) == Some(Some("lspsd.exe")) {
                        for d in outpath.iter() {
                            lspsd_exe_home.push(d);
                        }
                        let parent = lspsd_exe_home.parent().unwrap();
                        std::fs::create_dir_all(&parent)
                            .with_context(|| format!("cannot create dir {:?}", parent))?;
                        let mut outfile = std::fs::File::create(&lspsd_exe_home)
                            .with_context(|| format!("cannot create file {:?}", lspsd_exe_home))?;
                        io::copy(&mut file, &mut outfile).unwrap();
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}
