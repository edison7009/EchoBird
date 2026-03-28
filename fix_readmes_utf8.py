import os, re

taglines = {
    'en': {
        'h3': 'Deploy AI agents like a pro — no terminal, no config files, just one click.',
        'p': 'Install OpenClaw, Claude Code, ZeroClaw & more · Switch models across local and remote servers · All from one screen.'
    },
    'zh-CN': {
        'h3': '像 AI 专家一样部署 Agent —— 无需终端，无需配置，一键搞定。',
        'p': '一键安装 OpenClaw、Claude Code、ZeroClaw 等 · 本地服务器自由切换模型 · 一屏掌控所有 Agent。'
    },
    'zh-TW': {
        'h3': '像 AI 專家一樣部署 Agent —— 無需終端機，無需設定，一鍵搞定。',
        'p': '一鍵安裝 OpenClaw、Claude Code、ZeroClaw 等 · 本地伺服器自由切換模型 · 一屏掌控所有 Agent。'
    },
    'ja': {
        'h3': 'AI専門家のようにAgentをデプロイ — ターミナル不要、設定不要、ワンクリックで完了。',
        'p': 'OpenClaw、Claude Code、ZeroClawなどを一括インストール · ローカルサーバーでモデルを自由に切り替え · 1つの画面ですべてのAgentを管理。'
    },
    'ko': {
        'h3': 'AI 전문가처럼 Agent 배포 — 터미널이나 설정 없이 클릭 한 번으로 완료.',
        'p': 'OpenClaw, Claude Code, ZeroClaw 등 원클릭 설치 · 로컬 서버에서 자유로운 모델 전환 · 한 화면에서 모든 Agent 제어.'
    },
    'es': {
        'h3': 'Despliega agentes de IA como un profesional — sin terminal, sin configuración, con un solo clic.',
        'p': 'Instala OpenClaw, Claude Code, ZeroClaw y más con un clic · Cambia de modelo libremente en servidores locales · Controla todos los agentes desde una sola pantalla.'
    },
    'fr': {
        'h3': 'Déployez des agents IA comme un pro — sans terminal, sans configuration, en un seul clic.',
        'p': 'Installez OpenClaw, Claude Code, ZeroClaw et bien plus en un clic · Changez librement de modèle sur les serveurs locaux · Contrôlez tous les agents depuis un seul écran.'
    },
    'de': {
        'h3': 'KI-Agenten wie ein Profi einsetzen — ohne Terminal, ohne Konfiguration, mit nur einem Klick.',
        'p': 'OpenClaw, Claude Code, ZeroClaw und mehr mit einem Klick installieren · Modelle auf lokalen Servern frei wechseln · Alle Agenten auf einem Bildschirm steuern.'
    },
    'pt': {
        'h3': 'Implante agentes de IA como um profissional — sem terminal, sem configuração, com apenas um clique.',
        'p': 'Instale OpenClaw, Claude Code, ZeroClaw e muito mais com um clique · Mude de modelo livremente em servidores locais · Controle todos os agentes em uma única tela.'
    },
    'ru': {
        'h3': 'Развертывайте ИИ-агентов как профи — без терминала, без конфигурации, в один клик.',
        'p': 'Устанавливайте OpenClaw, Claude Code, ZeroClaw и другие в один клик · Свободно переключайте модели на локальных серверах · Управляйте всеми агентами на одном экране.'
    },
    'ar': {
        'h3': 'انشر وكلاء الذكاء الاصطناعي كالمحترفين — بدون محطة طرفية، بدون إعدادات، بنقرة واحدة.',
        'p': 'تثبيت OpenClaw و Claude Code و ZeroClaw والمزيد بنقرة واحدة · التبديل بين النماذج بحرية على الخوادم المحلية · التحكم في جميع الوكلاء من شاشة واحدة.'
    }
}

for f in os.listdir('d:/Echobird/docs'):
    if not f.startswith('README.') or not f.endswith('.md'):
        continue
        
    lang = f.replace('README.', '').replace('.md', '')
    if lang not in taglines:
        continue
        
    path = f'd:/Echobird/docs/{f}'
    
    with open(path, 'r', encoding='utf-8') as fh:
        c = fh.read()
        
    # 1. Update logo to icon.png
    c = re.sub(r'<img src="\./5\.png" alt="[^"]+" width="100%" />', r'<img src="../icon.png" alt="Echobird" width="120" />', c)
    
    # 2. Update taglines
    h3_p_regex = re.compile(
        r'<h3 align="center">.*?</h3>\s*<p align="center">\s*.*?(?:<br/>\s*<sub>.*?</sub>\s*)?</p>', 
        re.DOTALL
    )
    new_taglines = f'<h3 align="center">{taglines[lang]["h3"]}</h3>\n\n<p align="center">\n  {taglines[lang]["p"]}\n</p>'
    c = h3_p_regex.sub(new_taglines, c, count=1)
    
    # 3. Add 5.png before the features section
    features_regex = re.compile(r'---\s*\n\s*##\s*(?:✨\s*)?[^\n]+(?:\n\s*###\s*(?:🚀\s*)?[^\n]+)')
    
    if 'src="./5.png"' not in c:
        def insert_5png(match):
            return f'<p align="center">\n  <img src="./5.png" alt="Echobird Channels" width="100%" />\n</p>\n\n' + match.group(0)
        c = features_regex.sub(insert_5png, c, count=1)
        
    # 4. Remove remote LLM references
    # title 
    patterns_rm_title = [
        r' (?:or remotely|oder remote|ou a distance|o de forma remota|ou remotamente|ローカルまたはリモートで|로컬 또는 원격으로|локально или удаленно|محليًا أو عن بُعد|محليا أو عن بعد|或远程|及远程|或遠端|及遠端)',
    ]
    for pattern in patterns_rm_title:
        c = re.sub(pattern, '', c)
        
    # specific overrides
    rep_map = {
        '本地或远程运行': '本地运行',
        '本地及远程运行': '本地运行',
        '本地或遠端執行': '本地執行',
        'lokal oder remote ausführen': 'lokal ausführen',
        'localement ou à distance': 'localement',
        'localmente o de forma remota': 'localmente',
        'localmente ou remotamente': 'localmente',
        'локально или удаленно': 'локально',
        '로컬 또는 원격으로': '로컬로',
    }
    for old, new in rep_map.items():
        c = c.replace(old, new)
        
    # remove bullets
    lines = c.split('\n')
    new_lines = []
    for line in lines:
        if re.search(r'- \*\*(Remote LLM|远程 LLM|遠端 LLM|リモートLLM|원격 LLM|Удалённый LLM|LLM عن بُعد|LLM remoto|LLM distant|Remote-LLM)\*\*', line):
            continue
        new_lines.append(line)
    c = '\n'.join(new_lines)
    
    with open(path, 'w', encoding='utf-8', newline='') as fh:
        fh.write(c)
        
    print(f'Processed {f}')
