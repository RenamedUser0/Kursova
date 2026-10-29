
use chrono::{Duration, Local, NaiveDate};
use flate2::{write::GzEncoder, Compression};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

const DAYS: i64 = 14;

fn log(msg: &str) -> io::Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open("app.log")?;
    let msg = format!("[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
    writeln!(f, "{msg}")?;
    println!("{msg}");
    Ok(())
}

fn date(s: &str) -> Option<NaiveDate> {
    let d = s.get(..10)?;
    let parsed = NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
    (parsed.format("%Y-%m-%d").to_string() == d).then_some(parsed)
}

fn file_date(p: &Path) -> Option<NaiveDate> {
    date(p.file_name()?.to_str()?.strip_suffix(".log")?)
}

fn archive(src: &Path, dir: &Path) -> io::Result<()> {
    let name = src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Невідоме ім'я файлу")
    })?;

    let base = name.to_string_lossy();
    let mut dst = dir.join(format!("{base}.tar.gz"));
    let mut n = 1;

    while dst.exists() {
        dst = dir.join(format!("{base}.{n}.tar.gz"));
        n += 1;
    }

    let result = (|| {
        let enc = GzEncoder::new(File::create(&dst)?, Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(src, name)?;
        tar.into_inner()?.finish()?;
        fs::remove_file(src)?;
        Ok::<(), io::Error>(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&dst);
    } else {
        log(&format!("ARCHIVED: {} -> {}", src.display(), dst.display()))?;
    }

    result
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(input) = rfd::FileDialog::new()
        .set_title("Виберіть файл журналу")
        .add_filter("Log files", &["log", "txt"])
        .pick_file()
    else {
        println!("Вибір файлу скасовано");
        return Ok(());
    };

    let Some(root) = rfd::FileDialog::new()
        .set_title("Виберіть папку")
        .pick_folder()
    else {
        println!("Вибір папки скасовано");
        return Ok(());
    };

    let dir = root.join("logs");
    let archives = root.join("archives");

    if !input.is_file() {
        return Err("Вибраний файл не існує".into());
    }

    fs::create_dir_all(&dir)?;
    fs::create_dir_all(&archives)?;

    log("CustomLogRotator: Start")?;
    log(&format!("Вхідний файл: {}", input.display()))?;
    log(&format!("Папка журналів: {}", dir.display()))?;
    log(&format!("Папка архівів: {}", archives.display()))?;

    let temp = dir.join(".rotator_temp");

    if temp.exists() {
        fs::remove_dir_all(&temp)?;
    }
    fs::create_dir_all(&temp)?;

    let result = (|| -> io::Result<(u64, usize)> {
        let mut groups: BTreeMap<String, BufWriter<File>> = BTreeMap::new();
        let mut count = 0;

        for line in BufReader::new(File::open(&input)?).lines() {
            let line = line?;
            count += 1;

            let key = date(&line)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "undated".into());

            if !groups.contains_key(&key) {
                groups.insert(
                    key.clone(),
                    BufWriter::new(File::create(temp.join(format!("{key}.log")))?),
                );
            }

            writeln!(groups.get_mut(&key).unwrap(), "{line}")?;
        }

        for writer in groups.values_mut() {
            writer.flush()?;
        }

        Ok((count, groups.len()))
    })();

    let (lines, groups) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&temp);
            return Err(e.into());
        }
    };

    log(&format!("Оброблено рядків: {lines}; груп: {groups}"))?;

    let cutoff = Local::now().date_naive() - Duration::days(DAYS);
    let mut archived = 0;

    for entry in fs::read_dir(&temp)? {
        let src = entry?.path();
        let name = src.file_name().unwrap().to_string_lossy().into_owned();
        let dst = dir.join(&name);

        if file_date(&src).is_some_and(|d| d < cutoff) {
            archive(&src, &archives)?;
            archived += 1;
            continue;
        }

        if dst.exists() {
            fs::remove_file(&dst)?;
        }

        fs::rename(&src, &dst)?;
        log(&format!("CREATED: {}", dst.display()))?;
    }

    fs::remove_dir_all(&temp)?;

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        if entry.file_type()?.is_file()
            && file_date(&path).is_some_and(|d| d < cutoff)
        {
            match archive(&path, &archives) {
                Ok(()) => archived += 1,
                Err(e) => log(&format!("ERROR: {}: {e}", path.display()))?,
            }
        }
    }

    log(&format!("Створено архівів: {archived}"))?;
    log("CustomLogRotator: End")?;

    println!("\nРезультати збережено в:");
    println!("{}", root.display());

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ПОМИЛКА: {e}");
        let _ = log(&format!("ERROR: {e}"));
        std::process::exit(1);
    }
}
