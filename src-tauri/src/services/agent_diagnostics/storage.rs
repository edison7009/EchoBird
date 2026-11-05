use super::*;
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use rusty_leveldb::{LdbIterator, Options, DB};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    path: String,
    format: String,
    table: Option<String>,
    columns: Option<Vec<String>>,
    key: Option<String>,
    #[serde(default)]
    offset: usize,
}

struct CopyDir(PathBuf);
impl Drop for CopyDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_files(paths: Vec<PathBuf>) -> Result<CopyDir, String> {
    let dir =
        CopyDir(std::env::temp_dir().join(format!("echobird-diagnostic-{}", uuid::Uuid::new_v4())));
    super::super::native_account_files::private_dir(&dir.0)?;
    if paths.len() > 128 {
        return Err("Database snapshot exceeds 128 files".into());
    }
    let mut total = 0;
    let mut hashes = Vec::new();
    for path in &paths {
        if fs::symlink_metadata(path)
            .map_err(|e| e.to_string())?
            .is_symlink()
        {
            return Err("Database snapshot refuses linked files".into());
        }
        let bytes = read(path, FILE_LIMIT)?;
        total += bytes.len();
        if total > 64 * 1024 * 1024 {
            return Err("Database snapshot exceeds 64 MiB".into());
        }
        hashes.push(hash(&bytes));
        super::super::native_account_files::write(
            &dir.0.join(path.file_name().ok_or("Missing file name")?),
            &bytes,
        )?;
    }
    for (path, expected) in paths.iter().zip(hashes) {
        if hash(&read(path, FILE_LIMIT)?) != expected {
            return Err("Database changed during snapshot; pause the client and retry".into());
        }
    }
    Ok(dir)
}
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub(super) fn inspect(args: Args) -> Result<Value, String> {
    if args.offset > 100_000 {
        return Err("offset exceeds diagnostic limit".into());
    }
    let source = absolute(&args.path)?;
    match args.format.as_str() {
        "sqlite" => {
            if args.key.is_some() {
                return Err("key is only valid for LevelDB".into());
            }
            let mut paths = vec![source.clone()];
            let wal = PathBuf::from(format!("{}-wal", source.display()));
            if wal.exists() {
                paths.push(wal);
            }
            let copy = copy_files(paths)?;
            let conn = Connection::open_with_flags(
                copy.0.join(source.file_name().unwrap()),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(|e| e.to_string())?;
            conn.pragma_update(None, "query_only", true)
                .map_err(|e| e.to_string())?;
            let mut statement = conn
                .prepare("SELECT name, sql FROM sqlite_master WHERE type='table' ORDER BY name")
                .map_err(|e| e.to_string())?;
            let tables = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let Some(table) = args.table else {
                if args.columns.is_some() {
                    return Err("columns requires a table".into());
                }
                let rows: Vec<_> = tables
                    .iter()
                    .skip(args.offset)
                    .take(20)
                    .map(|(name, sql)| json!({"table":name,"sql":sql}))
                    .collect();
                return Ok(
                    json!({"path":source,"tables":rows,"total":tables.len(),"next_offset":if args.offset+rows.len()<tables.len(){Some(args.offset+rows.len())}else{None},"source_database_opened":false}),
                );
            };
            tables
                .iter()
                .find(|(name, _)| name == &table)
                .ok_or("Table not found")?;
            let table_type: String = conn
                .query_row(
                    "SELECT type FROM pragma_table_list WHERE schema='main' AND name=?1",
                    [&table],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if table_type != "table" {
                return Err("Virtual-table execution is outside read-only diagnostics".into());
            }
            let columns = args
                .columns
                .filter(|v| !v.is_empty() && v.len() <= 16)
                .ok_or("Select 1-16 explicit columns after inspecting schema")?;
            let selected = columns
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(",");
            let mut statement = conn
                .prepare(&format!(
                    "SELECT {selected} FROM {} LIMIT 21 OFFSET ?1",
                    quote(&table)
                ))
                .map_err(|e| e.to_string())?;
            // Reject SQLite's double-quoted-string fallback for unknown columns.
            let names = conn
                .prepare(&format!("PRAGMA table_info({})", quote(&table)))
                .map_err(|e| e.to_string())?
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            if columns.iter().any(|c| !names.contains(c)) {
                return Err("Unknown selected column".into());
            }
            let mut query = statement
                .query([args.offset as i64])
                .map_err(|e| e.to_string())?;
            let mut rows = Vec::new();
            while let Some(row) = query.next().map_err(|e| e.to_string())? {
                let mut values = serde_json::Map::new();
                for (i, name) in columns.iter().enumerate() {
                    let value = match row.get_ref(i).map_err(|e| e.to_string())? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(n) => json!(n),
                        ValueRef::Real(n) => json!(n),
                        ValueRef::Text(v) if v.len() <= 4096 => {
                            json!(std::str::from_utf8(v).map_err(|_| "Non-UTF8 SQLite text")?)
                        }
                        ValueRef::Text(v) | ValueRef::Blob(v) => {
                            json!({"bytes":v.len(),"sha256":hash(v)})
                        }
                    };
                    values.insert(name.clone(), value);
                }
                rows.push(Value::Object(values));
            }
            let more = rows.len() > 20;
            rows.truncate(20);
            Ok(
                json!({"path":source,"table":table,"rows":rows,"next_offset":if more{Some(args.offset+20)}else{None},"source_database_opened":false}),
            )
        }
        "leveldb" => {
            if args.table.is_some() || args.columns.is_some() {
                return Err("table/columns are only valid for SQLite".into());
            }
            if !source.join("CURRENT").is_file() {
                return Err("Existing LevelDB CURRENT is required; no database was created".into());
            }
            let paths = fs::read_dir(&source)
                .map_err(|e| e.to_string())?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|p| p.file_name().is_none_or(|n| n != "LOCK"))
                .filter(|p| !p.is_dir())
                .collect();
            let copy = copy_files(paths)?;
            let mut db = DB::open(
                &copy.0,
                Options {
                    create_if_missing: false,
                    paranoid_checks: true,
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
            if let Some(key) = args.key {
                let value = db.get(key.as_bytes()).map(|bytes| decode(&bytes, true));
                return Ok(
                    json!({"path":source,"key":key,"value":value,"found":value.is_some(),"source_database_opened":false}),
                );
            }
            let mut iterator = db.new_iter().map_err(|e| e.to_string())?;
            for _ in 0..args.offset {
                if iterator.next().is_none() {
                    break;
                }
            }
            let mut keys = Vec::new();
            for _ in 0..21 {
                if let Some((key, _)) = iterator.next() {
                    keys.push(decode(&key, false));
                } else {
                    break;
                }
            }
            let more = keys.len() > 20;
            keys.truncate(20);
            Ok(
                json!({"path":source,"keys":keys,"next_offset":if more{Some(args.offset+20)}else{None},"source_database_opened":false}),
            )
        }
        _ => Err("format must be sqlite or leveldb".into()),
    }
}
fn decode(bytes: &[u8], chromium: bool) -> Value {
    if bytes.len() > 4096 {
        return json!({"bytes":bytes.len(),"sha256":hash(bytes)});
    }
    if chromium && bytes.first() == Some(&1) {
        return json!({"encoding":"chromium-latin1","text":bytes[1..].iter().map(|b|char::from(*b)).collect::<String>()});
    }
    if chromium && bytes.first() == Some(&0) && (bytes.len() - 1).is_multiple_of(2) {
        if let Ok(text) = String::from_utf16(
            &bytes[1..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        ) {
            return json!({"encoding":"chromium-utf16","text":text});
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return json!({"encoding":"utf8","text":text});
    }
    json!({"encoding":"hex","hex":bytes.iter().map(|b|format!("{b:02x}")).collect::<String>()})
}
