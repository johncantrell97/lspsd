fn main() {
    if std::env::var_os("SKIP_DOWNLOAD").is_some() {
        return;
    }
    download::start().unwrap();
}

mod download {
    use anyhow::Context;
    use std::io::Cursor;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    include!("src/versions.rs");

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn download_filename() -> String {
        format!("lspsd-{}-aarch64-apple-darwin.zip", &VERSION)
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn download_filename() -> String {
        format!("lspsd-{}-x86_64-linux-gnu.zip", &VERSION)
    }

    pub(crate) fn start() -> anyhow::Result<()> {
        if std::env::var_os("LSPSD_SKIP_DOWNLOAD").is_some() {
            return Ok(());
        }
        let download_filename = download_filename();
        let out_dir = std::env::var_os("OUT_DIR").unwrap();

        let lspsd_exe_home = Path::new(&out_dir).join("lspsd");
        if !lspsd_exe_home.exists() {
            std::fs::create_dir(&lspsd_exe_home)
                .with_context(|| format!("cannot create dir {:?}", lspsd_exe_home))?;
        }
        let destination_filename = lspsd_exe_home.join("lspsd");

        if !destination_filename.exists() {
          let download_endpoint = std::env::var("LSPSD_DOWNLOAD_ENDPOINT").unwrap_or(
              "https://github.com/johncantrell97/lspsd/releases/download".to_owned(),
          );

          let url = format!("{}/{}/{}", download_endpoint, VERSION, download_filename);

          let downloaded_bytes = minreq::get(url).send().unwrap().into_bytes();

          let cursor = Cursor::new(downloaded_bytes);

          let mut archive = zip::ZipArchive::new(cursor).unwrap();
          let mut file = archive.by_index(0).unwrap();
          std::fs::create_dir_all(destination_filename.parent().unwrap()).unwrap();
          let mut outfile = std::fs::File::create(&destination_filename).unwrap();

          std::io::copy(&mut file, &mut outfile).unwrap();
          std::fs::set_permissions(
              &destination_filename,
              std::fs::Permissions::from_mode(0o755),
          )
          .unwrap();
        }
        Ok(())
    }
}
