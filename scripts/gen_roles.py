"""Regenerate roles-en.json and roles-zh-Hans.json from upstream repos.

IMPORTANT: Category iteration order MUST match role-prompts.txt image numbering.
Do NOT change CAT_ORDER without also re-mapping all images in docs/roles/*.png!
"""
import json, os, re

SKIP_DIRS = {'scripts', 'strategy', 'examples', '.github', '.git', 'coordination', 'playbooks', 'runbooks'}
SKIP_FILES = {'README.md', 'EXECUTIVE-BRIEF.md', 'QUICKSTART.md', 'CONTRIBUTING.md', 'nexus-strategy.md'}
IMG_BASE = 'https://echobird.ai/roles'

# Category iteration order — MUST match docs/roles/role-prompts.txt image numbering!
# New categories go at the END to avoid breaking existing image assignments.
CAT_ORDER = [
    'engineering', 'design', 'marketing', 'product', 'sales',
    'project-management', 'testing', 'support', 'game-development',
    'specialized', 'academic', 'paid-media', 'spatial-computing',
    # -- future categories below --
    'finance', 'hr', 'legal', 'supply-chain',
]

# Category display names
CAT_NAMES_EN = {
    'academic': 'Academic', 'design': 'Design', 'engineering': 'Engineering',
    'finance': 'Finance', 'game-development': 'Game Development', 'hr': 'HR',
    'legal': 'Legal', 'marketing': 'Marketing', 'paid-media': 'Paid Media',
    'product': 'Product', 'project-management': 'Project Management',
    'sales': 'Sales', 'spatial-computing': 'Spatial Computing',
    'specialized': 'Specialized', 'supply-chain': 'Supply Chain',
    'support': 'Support', 'testing': 'Testing',
}
CAT_NAMES_ZH = {
    'academic': '\u5b66\u672f\u90e8', 'design': '\u8bbe\u8ba1\u90e8', 'engineering': '\u5de5\u7a0b\u90e8',
    'finance': '\u8d22\u52a1\u90e8', 'game-development': '\u6e38\u620f\u5f00\u53d1\u90e8', 'hr': '\u4eba\u4e8b\u90e8',
    'legal': '\u6cd5\u52a1\u90e8', 'marketing': '\u8425\u9500\u90e8', 'paid-media': '\u4ed8\u8d39\u5a92\u4f53\u90e8',
    'product': '\u4ea7\u54c1\u90e8', 'project-management': '\u9879\u76ee\u7ba1\u7406\u90e8',
    'sales': '\u9500\u552e\u90e8', 'spatial-computing': '\u7a7a\u95f4\u8ba1\u7b97\u90e8',
    'specialized': '\u4e13\u9879\u90e8', 'supply-chain': '\u4f9b\u5e94\u94fe\u90e8',
    'support': '\u652f\u6301\u90e8', 'testing': '\u6d4b\u8bd5\u90e8',
}

def parse_frontmatter(content):
    """Extract name and description from YAML frontmatter."""
    m = re.match(r'^---\s*\n(.*?)\n---', content, re.DOTALL)
    if not m:
        return None, None
    fm = m.group(1)
    name = None
    desc = None
    for line in fm.split('\n'):
        if line.startswith('name:'):
            name = line[5:].strip().strip('"').strip("'")
        elif line.startswith('description:'):
            desc = line[12:].strip().strip('"').strip("'")
    return name, desc

def scan_repo(repo_dir, cat_names, locale_prefix):
    roles = []
    categories_found = set()

    # Iterate categories in FIXED order (not alphabetical!) to preserve image numbering
    for cat_dir in CAT_ORDER:
        cat_path = os.path.join(repo_dir, cat_dir)
        if not os.path.isdir(cat_path):
            continue
        if cat_dir not in cat_names:
            continue

        # Recursively scan category directory (handles nested dirs like game-development/blender/)
        for dirpath, dirnames, filenames in os.walk(cat_path):
            dirnames.sort()  # ensure consistent subdir order
            for md_file in sorted(filenames):
                if not md_file.endswith('.md') or md_file in SKIP_FILES:
                    continue

                filepath = os.path.join(dirpath, md_file)
                try:
                    with open(filepath, 'r', encoding='utf-8') as f:
                        content = f.read()
                except:
                    continue

                name, desc = parse_frontmatter(content)
                role_id = md_file.replace('.md', '')

                if not name:
                    name = role_id.replace('-', ' ').title()
                if not desc:
                    desc = name

                # filePath is relative to repo root (e.g. game-development/blender/blender-addon-engineer.md)
                rel_path = os.path.relpath(filepath, repo_dir).replace('\\', '/')

                # img = same path as filePath but .md → .png, under locale prefix
                img_path = f'{locale_prefix}/{rel_path}'.replace('.md', '.png')

                categories_found.add(cat_dir)
                roles.append({
                    'id': role_id,
                    'name': name,
                    'description': desc,
                    'category': cat_dir,
                    'filePath': rel_path,
                    'img': f'{IMG_BASE}/{img_path}',
                })

    # Also pick up any NEW categories not yet in CAT_ORDER (appended at end)
    for cat_dir in sorted(os.listdir(repo_dir)):
        if cat_dir in SKIP_DIRS or cat_dir in [c for c in CAT_ORDER]:
            continue
        cat_path = os.path.join(repo_dir, cat_dir)
        if not os.path.isdir(cat_path) or cat_dir not in cat_names:
            continue
        for dirpath, dirnames, filenames in os.walk(cat_path):
            dirnames.sort()
            for md_file in sorted(filenames):
                if not md_file.endswith('.md') or md_file in SKIP_FILES:
                    continue
                filepath = os.path.join(dirpath, md_file)
                try:
                    with open(filepath, 'r', encoding='utf-8') as f:
                        content = f.read()
                except:
                    continue
                name, desc = parse_frontmatter(content)
                role_id = md_file.replace('.md', '')
                if not name:
                    name = role_id.replace('-', ' ').title()
                if not desc:
                    desc = name
                rel_path = os.path.relpath(filepath, repo_dir).replace('\\', '/')
                img_path = f'{locale_prefix}/{rel_path}'.replace('.md', '.png')
                categories_found.add(cat_dir)
                roles.append({
                    'id': role_id, 'name': name, 'description': desc,
                    'category': cat_dir, 'filePath': rel_path,
                    'img': f'{IMG_BASE}/{img_path}',
                })

    # Build categories list in FIXED order (matching role output order)
    cat_list = []
    for cat_id in CAT_ORDER:
        if cat_id in categories_found:
            cat_list.append({'id': cat_id, 'name': cat_names.get(cat_id, cat_id)})
    # Append any extra categories not in CAT_ORDER
    for cat_id in sorted(categories_found):
        if cat_id not in CAT_ORDER:
            cat_list.append({'id': cat_id, 'name': cat_names.get(cat_id, cat_id)})

    return {'categories': cat_list, 'roles': roles}

# EN
en_data = scan_repo(r'D:\tmp\agency-agents', CAT_NAMES_EN, 'en')
with open(r'D:\Echobird\docs\roles\roles-en.json', 'w', encoding='utf-8') as f:
    json.dump(en_data, f, ensure_ascii=False, indent=2)
print(f"EN: {len(en_data['roles'])} roles, {len(en_data['categories'])} categories")

# ZH
zh_data = scan_repo(r'D:\tmp\agency-agents-zh', CAT_NAMES_ZH, 'zh-Hans')
with open(r'D:\Echobird\docs\roles\roles-zh-Hans.json', 'w', encoding='utf-8') as f:
    json.dump(zh_data, f, ensure_ascii=False, indent=2)
print(f"ZH: {len(zh_data['roles'])} roles, {len(zh_data['categories'])} categories")
