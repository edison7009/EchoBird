import os, re

for f in os.listdir('d:/Echobird/docs'):
    if not (f.startswith('README.') and f.endswith('.md')):
        continue
    path = f'd:/Echobird/docs/{f}'
    
    with open(path, 'r', encoding='utf-8') as fh:
        c = fh.read()
        
    changed = False
    
    # 1. Fix icon path
    new_c = c.replace('<img src="../icon.png"', '<img src="./icon.png"')
    if new_c != c:
        c = new_c
        changed = True
        
    # 2. Add 4.png to Screenshots section if missing
    if '4.png' not in c:
        # Find the localized Channels text from the features section
        # Look for ### 📡 Channels — (some text)
        channels_match = re.search(r'### 📡\s*(Channels(?:[^-\n]*)—\s*[^\n]+)', c)
        if channels_match:
            channels_text = channels_match.group(1).strip()
            # If "Channels" is not at the start, just use the whole matched line without emojis
            # Replace the generic english fallback if found
        else:
            channels_text = "Channels — Control multiple agents from one screen"
            
        # find the insertion point: after ![Local Server](./3.png) (or whatever the 3.png line is)
        insert_regex = re.compile(r'(!\[[^\]]+\]\(\./3\.png\)\n\n)(---)')
        
        replacement = r'\1' + f'### {channels_text}\n![Channels](./4.png)\n\n' + r'\2'
        new_c_2 = insert_regex.sub(replacement, c)
        
        if new_c_2 != c:
            c = new_c_2
            changed = True
            
    if changed:
        with open(path, 'w', encoding='utf-8', newline='') as fh:
            fh.write(c)
        print(f'Fixed {f}')
