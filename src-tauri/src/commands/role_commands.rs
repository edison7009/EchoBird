// Role scanner — reads agency-agents repos, parses YAML frontmatter
// Supports locale-based directory selection (roles/en/, roles/zh-Hans/)
// Category labels read from _categories.json — add new language = add file, zero code changes


use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Category labels from _categories.json ──

fn load_category_labels(locale_dir: &std::path::Path) -> HashMap<String, String> {
    let path = locale_dir.join("_categories.json");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) {
            return map;
        }
    }
    log::warn!("[Roles] _categories.json not found in {:?}, using directory names", locale_dir);
    HashMap::new()
}

// ── Category sort order ──

fn category_order(dir_name: &str) -> u32 {
    match dir_name {
        "engineering" => 0,
        "design" => 1,
        "marketing" => 2,
        "product" => 3,
        "sales" => 4,
        "paid-media" => 5,
        "project-management" => 6,
        "testing" => 7,
        "support" => 8,
        "game-development" => 9,
        "spatial-computing" => 10,
        "specialized" => 11,
        "academic" => 12,
        _ => 99,
    }
}

// ── Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCategory {
    pub id: String,        // directory name: "engineering"
    pub label: String,     // display: "工程部"
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleEntry {
    pub id: String,            // filename without .md: "engineering-frontend-developer"
    pub name: String,          // from frontmatter: "前端开发者"
    pub description: String,   // from frontmatter
    pub category: String,      // directory name: "engineering"
    pub color: Option<String>, // from frontmatter
    pub emoji: Option<String>, // from frontmatter
    pub file_path: String,     // relative: "engineering/engineering-frontend-developer.md"
    pub img: Option<String>,   // placeholder image path
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleScanResult {
    pub categories: Vec<RoleCategory>,
    pub roles: Vec<RoleEntry>,
    pub locale: String,
    pub all_label: String,     // "全部" / "All" — from _categories.json "all" key
}

// ── Frontmatter parser ──

fn parse_frontmatter(content: &str) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let mut name = None;
    let mut description = None;
    let mut color = None;
    let mut emoji = None;

    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let fm = &content[3..3 + end];
            for line in fm.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
                } else if let Some(val) = line.strip_prefix("color:") {
                    color = Some(val.trim().to_string());
                } else if let Some(val) = line.strip_prefix("emoji:") {
                    emoji = Some(val.trim().to_string());
                }
            }
        }
    }

    (name, description, color, emoji)
}

// ── Roles directory resolution ──

fn roles_dir() -> PathBuf {
    // Same traversal strategy as plugins_dir — look near the executable and project root
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut candidates = vec![
        exe_dir.join("roles"),
        exe_dir.join("_up_").join("roles"),
    ];

    if let Some(p1) = exe_dir.parent() {
        candidates.push(p1.join("roles"));
        candidates.push(p1.join("_up_").join("roles"));
        candidates.push(p1.join("Resources").join("roles"));
        if let Some(p2) = p1.parent() {
            candidates.push(p2.join("roles"));
            if let Some(p3) = p2.parent() {
                candidates.push(p3.join("roles"));
            }
        }
    }

    // Dev mode
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(project_root) = manifest_dir.parent() {
        candidates.push(project_root.join("roles"));
    }

    candidates.push(PathBuf::from("roles"));

    for candidate in &candidates {
        if candidate.exists() {
            log::info!("[Roles] Found roles dir: {:?}", candidate);
            return candidate.clone();
        }
    }

    log::warn!("[Roles] No roles directory found");
    PathBuf::from("roles")
}

// ── Placeholder images (cycle through available images) ──

fn placeholder_images() -> Vec<String> {
    vec![
        "1119642287_IGDB-285x380.jpg".into(),
        "116747788-285x380.png".into(),
        "1329153872_IGDB-285x380.jpg".into(),
        "1435206302_IGDB-285x380.jpg".into(),
        "1597660489_IGDB-285x380.jpg".into(),
        "1630982727_IGDB-285x380.jpg".into(),
        "21779-285x380.jpg".into(),
        "29307_IGDB-285x380.jpg".into(),
        "509538_IGDB-285x380.jpg".into(),
        "509658-285x380.jpg".into(),
        "511224-285x380.jpg".into(),
        "512864_IGDB-285x380.jpg".into(),
        "513181_IGDB-285x380.jpg".into(),
        "515025-285x380.png".into(),
        "516575-285x380.png".into(),
        "55453844_IGDB-285x380.jpg".into(),
        "66082-285x380.jpg".into(),
        "66366_IGDB-285x380.jpg".into(),
    ]
}

// ── Scan Commands ──

/// Scan roles directory for a given locale, returning categories and role entries
#[tauri::command]
pub fn scan_roles(locale: String) -> RoleScanResult {
    let base = roles_dir();
    
    // Map locale to directory: "zh-Hans", "zh-Hant", "zh" → "zh-Hans"; else "en"
    let dir_name = if locale.starts_with("zh") { "zh-Hans" } else { "en" };
    let locale_dir = base.join(dir_name);

    if !locale_dir.exists() {
        log::warn!("[Roles] Locale dir not found: {:?}", locale_dir);
        return RoleScanResult {
            categories: vec![],
            roles: vec![],
            locale: dir_name.to_string(),
            all_label: "All".to_string(),
        };
    }

    // Load category labels from _categories.json
    let cat_labels = load_category_labels(&locale_dir);
    let all_label = cat_labels.get("all").cloned().unwrap_or_else(|| "All".to_string());

    let images = placeholder_images();
    let mut categories: Vec<RoleCategory> = Vec::new();
    let mut roles: Vec<RoleEntry> = Vec::new();
    let mut img_idx: usize = 0;

    // Scan subdirectories (each is a category)
    let skip_dirs = ["examples", "integrations", "scripts", "strategy", ".git", ".github"];

    let mut dirs: Vec<_> = std::fs::read_dir(&locale_dir)
        .unwrap_or_else(|_| std::fs::read_dir(".").unwrap())
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            !skip_dirs.contains(&name.as_str())
        })
        .collect();

    dirs.sort_by_key(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        category_order(&name)
    });

    for dir_entry in &dirs {
        let cat_name = dir_entry.file_name().to_string_lossy().to_string();
        
        // Get label from _categories.json, fallback to directory name
        let label = cat_labels.get(&cat_name).cloned().unwrap_or_else(|| cat_name.clone());
        categories.push(RoleCategory {
            id: cat_name.clone(),
            label,
            order: category_order(&cat_name),
        });

        // Scan .md files in this category (including subdirectories for game-development)
        scan_md_files(&dir_entry.path(), &cat_name, &images, &mut img_idx, &mut roles, &locale_dir);
    }

    log::info!("[Roles] Scanned {}: {} categories, {} roles", dir_name, categories.len(), roles.len());

    RoleScanResult {
        categories,
        roles,
        locale: dir_name.to_string(),
        all_label,
    }
}

fn scan_md_files(
    dir: &std::path::Path,
    category: &str,
    images: &[String],
    img_idx: &mut usize,
    roles: &mut Vec<RoleEntry>,
    base_dir: &std::path::Path,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Recurse into subdirs (e.g. game-development/unity/)
            scan_md_files(&path, category, images, img_idx, roles, base_dir);
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let filename = path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Skip README and non-role files
        if filename == "README" || filename == "CONTRIBUTING" || filename == "UPSTREAM" {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (name, description, color, emoji) = parse_frontmatter(&content);

        // Skip files without frontmatter name
        let name = match name {
            Some(n) => n,
            None => continue,
        };

        let rel_path = path.strip_prefix(base_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let img = format!("/role/{}", images[*img_idx % images.len()]);
        *img_idx += 1;

        roles.push(RoleEntry {
            id: filename,
            name,
            description: description.unwrap_or_default(),
            category: category.to_string(),
            color,
            emoji,
            file_path: rel_path,
            img: Some(img),
        });
    }
}

/// Load the full content of a role .md file (for system_prompt injection)
#[tauri::command]
pub fn load_role_content(locale: String, file_path: String) -> Result<String, String> {
    let base = roles_dir();
    let dir_name = if locale.starts_with("zh") { "zh-Hans" } else { "en" };
    let full_path = base.join(dir_name).join(&file_path);

    if !full_path.exists() {
        return Err(format!("Role file not found: {}", file_path));
    }

    std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Failed to read role file: {}", e))
}
