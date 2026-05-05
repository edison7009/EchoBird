// i18n type definitions (separate file to avoid circular imports)

// Translation key definitions
export type TKey =
    // Navigation
    | 'nav.modelNexus' | 'nav.appManager'
    | 'nav.localServer' | 'nav.motherAgent'
    | 'nav.news' | 'nav.projects' | 'nav.courses'
    // Page titles
    | 'page.modelNexus' | 'page.appManager'
    | 'page.localServer' | 'page.motherAgent'
    | 'page.news' | 'page.projects' | 'page.courses'
    // AI Courses
    | 'courses.filter' | 'courses.cat.all'
    // AI Pulse (News + Projects shared)
    | 'pulse.archive' | 'pulse.days' | 'pulse.fetchFailed'
    | 'pulse.loadingFirst' | 'pulse.empty'
    // Settings
    | 'settings.title' | 'settings.version' | 'settings.language' | 'settings.updates'
    | 'settings.checkForUpdates' | 'settings.checking'
    | 'settings.latestVersion' | 'settings.checkFailed'
    | 'settings.appearance' | 'settings.themeLight' | 'settings.themeDark' | 'settings.themeSystem'
    // Buttons
    | 'btn.addModel' | 'btn.refresh'
    | 'btn.cancel' | 'btn.delete' | 'btn.edit'
    | 'btn.launchApp'
    | 'btn.modifyOnly' | 'btn.start' | 'btn.stop'
    | 'btn.add' | 'btn.remove'
    | 'btn.copy' | 'btn.copied'
    // Status
    | 'status.running' | 'status.offline' | 'status.scanning'
    | 'status.complete' | 'status.failed'

    // Model dialog
    | 'model.name' | 'model.apiKey' | 'model.modelId'
    | 'model.openaiUrl' | 'model.anthropicUrl' | 'model.proxyNode'
    | 'model.editConfig' | 'model.proxyTunnel' | 'model.specificProxy'
    | 'model.deleteTitle' | 'model.deleteConfirm'
    | 'model.selectToTest' | 'model.escCancel' | 'model.enterSave'
    // ModelCard labels
    | 'model.label' | 'model.source' | 'model.latency' | 'model.debugTesting' | 'model.notTested' | 'model.providers' | 'model.relays'
    | 'model.cloud' | 'model.local' | 'model.tunnel'

    // App Manager
    | 'agent.myLocalModel' | 'agent.selectTool' | 'agent.selectModelFor'
    | 'agent.noModelsTitle' | 'agent.noModelsHintPre' | 'agent.noModelsHintPost'
    | 'agent.applyAndLaunch' | 'agent.appliedVia' | 'agent.noModelConfig'
    | 'agent.modelsTab'
    | 'agent.installViaMother'
    | 'agent.restore'
    // Tool categories
    | 'toolCat.all' | 'toolCat.agentOS' | 'toolCat.ide' | 'toolCat.cli'
    | 'toolCat.autoTrading' | 'toolCat.game' | 'toolCat.desktop' | 'toolCat.utility'
    // ToolCard labels
    | 'tool.models'
    | 'tool.app' | 'tool.config' | 'tool.version'
    // Local Server
    | 'server.selectModel' | 'server.context' | 'server.port' | 'server.runtime'
    | 'server.removeDirectories' | 'server.removeDirectoryConfirm'
    | 'server.compute' | 'server.stdout'
    | 'server.selectFromPanel' | 'server.awaitingInit' | 'server.selectConfigStart'
    | 'server.local' | 'server.store'
    | 'server.selectModelDir' | 'server.downloadFromStore'
    | 'server.gpuFull' | 'server.cpuOnly'
    | 'server.setupEngine' | 'server.upgradeEngine' | 'server.downloading'
    // Debug (used in ModelNexus)
    | 'debug.console' | 'debug.gettingStarted'    // Download / Model Store
    | 'download.location' | 'download.changePath' | 'download.selectNewDir'
    | 'download.inQueue' | 'download.pause' | 'download.resume'
    | 'download.cancel' | 'download.retry'
    | 'quant.light' | 'quant.standard' | 'quant.extended' | 'quant.large' | 'quant.maximum'
    // ModelStore buttons
    | 'store.add' | 'store.del' | 'store.cancel' | 'store.remove'
    | 'store.ver' | 'store.ready'
    // VRAM fitness labels
    | 'vram.easy' | 'vram.good' | 'vram.tight' | 'vram.heavy'
    // Common
    | 'common.noData' | 'common.confirm' | 'common.website'
    | 'common.areYouSure' | 'common.inputting'
    // API Key encryption status
    | 'key.encrypted' | 'key.destroyed'
    // Developer invite hint
    | 'hint.devInvite'
    // Mother Agent
    | 'mother.selectModel'
    | 'mother.hintInstall' | 'mother.hintShowSpecs' | 'mother.hintShowSpecsLocal' | 'mother.hintTroubleshoot' | 'mother.hintUninstall'
    | 'mother.hintNetworkInfo' | 'mother.hintSecurityAudit'
    | 'mother.enterMessage' | 'mother.noModels'
    | 'mother.servers' | 'mother.sshGuide' | 'mother.local' | 'mother.noServer'
    | 'mother.addServer' | 'mother.hostIp' | 'mother.port' | 'mother.username'
    | 'mother.passwordKey' | 'mother.hostPlaceholder' | 'mother.userPlaceholder'
    | 'mother.passwordPlaceholder' | 'mother.encrypted'
    | 'mother.testing' | 'mother.testConnection'
    | 'mother.cancel' | 'mother.addServerBtn'
    | 'mother.deleteServerTitle' | 'mother.deleteServerMsg'
    | 'mother.displayName' | 'mother.optional' | 'mother.displayNamePlaceholder'
    // SSH Guide
    | 'ssh.cloudDesc' | 'ssh.usernameHint' | 'ssh.passwordHint' | 'ssh.ipHint' | 'ssh.portHint'
    | 'ssh.cloudUsername' | 'ssh.cloudPassword' | 'ssh.cloudIp'
    | 'ssh.winStep1' | 'ssh.winStep2' | 'ssh.winStep3' | 'ssh.winStep4'
    | 'ssh.winUsername' | 'ssh.winPassword' | 'ssh.winIp' | 'ssh.winNote'
    | 'ssh.macStep' | 'ssh.macOr'
    | 'ssh.macUsername' | 'ssh.macPassword' | 'ssh.macIp'
    | 'ssh.linuxNote'
    | 'ssh.linuxUsername' | 'ssh.linuxPassword' | 'ssh.linuxIp'
    | 'ssh.termuxUsername' | 'ssh.termuxPassword' | 'ssh.termuxIp'
    | 'ssh.ishUsername' | 'ssh.ishPassword' | 'ssh.ishIp'
    // Mother Agent connection error messages
    | 'mother.connectionRetrying' | 'mother.connectionFailed' | 'mother.connectionHint'
    // Error messages (user-friendly, non-technical)
    | 'error.connectionTimeout' | 'error.serverUnreachable' | 'error.agentFailed'
    | 'error.noServerConfig' | 'error.noModelSelected' | 'error.requestFailed'
    | 'error.userCancelled'
    | 'common.inputting'
    // App brand
    | 'app.name';

export type Translations = Record<TKey, string>;
