use serde_json::Value;
use std::path::PathBuf;

pub(super) fn native_locale() -> String {
    let locale = super::super::claude_desktop_accounts::data_dir()
        .ok()
        .and_then(|directory| {
            std::fs::read(super::super::claude_desktop_accounts::native_config_path(
                &directory,
            ))
            .ok()
        })
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|config| config["locale"].as_str().map(str::to_owned));
    normalize(locale.as_deref().unwrap_or("en-US")).to_owned()
}

fn normalize(locale: &str) -> &str {
    match locale {
        "zh-CN" | "zh-cn" | "zh-Hans" => "zh-Hans",
        value
            if !value.is_empty()
                && value.len() <= 32
                && value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-') =>
        {
            value
        }
        _ => "en-US",
    }
}

fn catalog_path(path: &str) -> Option<PathBuf> {
    let relative = match path {
        "/i18n/zh-Hans.json" | "/i18n/s/zh-Hans.json" => "zh-Hans.json",
        "/i18n/dynamic/zh-Hans.json" => "dynamic/zh-Hans.json",
        _ => return None,
    };
    Some(
        super::super::tool_manager::find_tools_dir()?
            .join("claudedesktop/locales")
            .join(relative),
    )
}

pub(super) fn catalog(path: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(path) = catalog_path(path) else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err("Invalid Claude language catalog".into());
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(super) fn html(bytes: Vec<u8>, locale: &str) -> Result<Vec<u8>, String> {
    if locale != "zh-Hans" {
        return Ok(bytes);
    }
    let html = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    let settings = serde_json::to_string(locale).map_err(|error| error.to_string())?;
    let has_chinese_catalog = html
        .split("data-i18n-catalogs=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .is_some_and(|catalogs| {
            catalogs
                .split(',')
                .any(|catalog| catalog.starts_with("zh-Hans:"))
        });
    let catalog = if has_chinese_catalog {
        ""
    } else {
        "<meta name=\"i18n-catalogs\" data-i18n-catalogs=\"zh-Hans:1.1\" data-i18n-released=\"zh-Hans\">"
    };
    let initialization = format!(
        "{catalog}<script>try{{localStorage.setItem('spa:locale',{settings});localStorage.setItem('spa:locale-source','user_choice');document.documentElement.lang={settings};}}catch{{}}</script>"
    );
    let start = html
        .find("<head")
        .filter(|start| {
            html.as_bytes()
                .get(start + 5)
                .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
        })
        .ok_or("Claude shell document has no head")?;
    let end = html[start..]
        .find('>')
        .ok_or("Claude shell head is incomplete")?
        + start
        + 1;
    let mut html = html;
    html.insert_str(end, &initialization);
    Ok(html.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_native_language_and_initializes_chinese_before_the_application() {
        assert_eq!(normalize("zh-CN"), "zh-Hans");
        assert_eq!(normalize("ja-JP"), "ja-JP");
        assert_eq!(normalize("</script>"), "en-US");
        let original =
            b"<html><head><script src=\"/assets/app.js\"></script></head></html>".to_vec();
        let chinese = String::from_utf8(html(original.clone(), "zh-Hans").unwrap()).unwrap();
        assert!(chinese.find("spa:locale").unwrap() < chinese.find("/assets/app.js").unwrap());
        assert!(chinese.contains("zh-Hans:1.1"));
        let public = b"<html><head data-ion-ip-country=\"SG\"><script src=\"app.js\"></script></head></html>".to_vec();
        let translated = String::from_utf8(html(public, "zh-Hans").unwrap()).unwrap();
        assert!(translated.contains("<head data-ion-ip-country=\"SG\">"));
        assert!(translated.find("spa:locale").unwrap() < translated.find("app.js").unwrap());
        let public = b"<html><head><meta name=\"i18n-catalogs\" data-i18n-catalogs=\"de-DE:123,zh-Hans:current\"></head></html>".to_vec();
        let translated = String::from_utf8(html(public, "zh-Hans").unwrap()).unwrap();
        assert_eq!(translated.matches("name=\"i18n-catalogs\"").count(), 1);
        assert!(translated.contains("zh-Hans:current"));
        assert_eq!(html(original.clone(), "en-US").unwrap(), original);
        let dictionary: Value =
            serde_json::from_slice(&catalog("/i18n/s/zh-Hans.json").unwrap().unwrap()).unwrap();
        assert!(dictionary.as_object().unwrap().len() > 10000);
        assert_eq!(dictionary["Vhm1tObTnr"], "统计");
        assert_eq!(dictionary["sS1fYP0r5H"], "全部时间");
        assert_eq!(dictionary["2Q4imNsBCu"], "新建");
        assert!(catalog("/i18n/../private.json").unwrap().is_none());
    }
}
