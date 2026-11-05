use super::*;
use std::io::{Seek, SeekFrom};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    path: String,
    member: Option<String>,
    search: Option<String>,
    #[serde(default)]
    offset: usize,
}

pub(super) fn inspect(args: Args) -> Result<Value, String> {
    let path = absolute(&args.path)?;
    if !fs::metadata(&path).map_err(|e| e.to_string())?.is_file() {
        return Err("Expected a regular ASAR file".into());
    }
    let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let mut prefix = [0u8; 16];
    file.read_exact(&mut prefix)
        .map_err(|_| "Truncated ASAR header")?;
    let block = u32::from_le_bytes(prefix[4..8].try_into().unwrap()) as u64;
    let size = u32::from_le_bytes(prefix[12..16].try_into().unwrap()) as u64;
    if prefix[..4] != 4u32.to_le_bytes()
        || !(8..=FILE_LIMIT).contains(&block)
        || size > block - 8
        || block + 8 > length
    {
        return Err("Invalid ASAR header bounds".into());
    }
    let mut header = vec![0; size as usize];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let tree: Value = serde_json::from_slice(&header).map_err(|e| e.to_string())?;
    if let Some(member) = args.member {
        if member.is_empty()
            || member
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == ".." || s.contains('\\'))
        {
            return Err("Invalid ASAR member path".into());
        }
        let mut node = &tree;
        for part in member.split('/') {
            node = node
                .get("files")
                .and_then(|v| v.get(part))
                .ok_or("ASAR member not found")?;
        }
        if node.get("link").is_some() || node["unpacked"] == true || node.get("files").is_some() {
            return Err("Select a packed, non-link text member".into());
        }
        let offset = node["offset"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("Invalid ASAR offset")?;
        let size = node["size"].as_u64().ok_or("Invalid ASAR member size")?;
        let base = (block + 8)
            .checked_add(offset)
            .ok_or("ASAR offset overflow")?;
        if base.checked_add(size).is_none_or(|end| end > length) || args.offset as u64 > size {
            return Err("ASAR member outside archive bounds".into());
        }
        let count = (size - args.offset as u64).min(if args.search.is_some() {
            1024 * 1024
        } else {
            6004
        });
        file.seek(SeekFrom::Start(base + args.offset as u64))
            .map_err(|e| e.to_string())?;
        let mut bytes = vec![0; count as usize];
        file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        // Align byte pages to complete UTF-8 characters without rewriting the source.
        let mut start = 0;
        while start < bytes.len() && bytes[start] & 0xc0 == 0x80 {
            start += 1;
        }
        if start > 3 || (args.offset == 0 && start > 0) {
            return Err("Member page is not UTF-8 text".into());
        }
        let mut end = bytes.len();
        let text = loop {
            match std::str::from_utf8(&bytes[start..end]) {
                Ok(text) => break text,
                Err(e) if e.error_len().is_none() && end > start => end -= 1,
                Err(_) => return Err("Member page is not UTF-8 text".into()),
            }
        };
        let next = args.offset + end;
        if let Some(search) = args.search {
            if search.is_empty() || search.len() > 256 {
                return Err("search must contain 1-256 UTF-8 bytes".into());
            }
            let matches: Vec<_> = text
                .match_indices(&search)
                .take(20)
                .map(|(at, _)| {
                    let mut left = at.saturating_sub(100);
                    while !text.is_char_boundary(left) {
                        left += 1;
                    }
                    let mut right = (at + search.len() + 100).min(text.len());
                    while !text.is_char_boundary(right) {
                        right -= 1;
                    }
                    json!({"offset":args.offset+start+at,"context":&text[left..right]})
                })
                .collect();
            let capped = text.match_indices(&search).nth(20).is_some();
            let mut resume = if capped {
                matches.last().unwrap()["offset"].as_u64().unwrap() as usize + search.len()
            } else {
                next.saturating_sub(search.len().saturating_sub(1))
                    .max(args.offset + start)
            };
            while resume > args.offset + start
                && !text.is_char_boundary(resume - args.offset - start)
            {
                resume -= 1;
            }
            return Ok(
                json!({"path":path,"member":member,"matches":matches,"scanned_bytes":end-start,"complete":!capped && next as u64 == size,"next_offset":if capped || (next as u64)<size {Some(resume)}else{None}}),
            );
        }
        let mut end = text.len().min(6000);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let next = args.offset + start + end;
        return Ok(
            json!({"path":path,"member":member,"size":size,"offset":args.offset+start,"text":&text[..end],"next_offset":if (next as u64)<size {Some(next)}else{None},"executed":false}),
        );
    }
    if args.search.is_some() {
        return Err("search requires an exact member".into());
    }
    let mut entries = Vec::new();
    fn walk(node: &Value, prefix: &str, entries: &mut Vec<Value>) -> Result<(), String> {
        if entries.len() >= 100_000 {
            return Err("ASAR index exceeds diagnostic limit".into());
        }
        if let Some(files) = node.get("files").and_then(Value::as_object) {
            for (name, child) in files {
                walk(child, &format!("{prefix}{name}/"), entries)?;
            }
        } else {
            entries.push(json!({"member":prefix.trim_end_matches('/'),"size":node["size"],"unpacked":node["unpacked"]==true,"link":node["link"]}));
        }
        Ok(())
    }
    if !tree["files"].is_object() {
        return Err("ASAR has no file index".into());
    }
    walk(&tree, "", &mut entries)?;
    let next = args.offset.saturating_add(40).min(entries.len());
    if args.offset > entries.len() {
        return Err("Index offset exceeds entry count".into());
    }
    Ok(
        json!({"path":path,"total_entries":entries.len(),"entries":&entries[args.offset..next],"next_offset":if next<entries.len(){Some(next)}else{None},"executed":false}),
    )
}
