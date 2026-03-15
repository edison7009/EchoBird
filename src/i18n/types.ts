// i18n type definitions (separate file to avoid circular imports)

// Translation key definitions
export type TKey =
    // Navigation
    | 'nav.modelNexus' | 'nav.skillBrowser' | 'nav.appManager'
    | 'nav.localServer' | 'nav.motherAgent' | 'nav.channels'
    // Page titles
    | 'page.modelNexus' | 'page.skillBrowser' | 'page.appManager'
    | 'page.localServer' | 'page.motherAgent' | 'page.channels'
    // Settings
    | 'settings.title' | 'settings.version' | 'settings.language' | 'settings.updates'
    | 'settings.checkForUpdates' | 'settings.checking'
    | 'settings.latestVersion' | 'settings.checkFailed'
    | 'settings.closeBehavior' | 'settings.closeAsk' | 'settings.closeMinimize' | 'settings.closeQuit'
    // Buttons
    | 'btn.addModel' | 'btn.apply' | 'btn.scanAgain' | 'btn.refresh'
    | 'btn.save' | 'btn.cancel' | 'btn.delete' | 'btn.edit'
    | 'btn.install' | 'btn.uninstall' | 'btn.launchApp' | 'btn.loading'
    | 'btn.open' | 'btn.modifyOnly' | 'btn.start' | 'btn.stop'
    | 'btn.add' | 'btn.remove' | 'btn.saveModel' | 'btn.compute'
    | 'btn.sendLogs' | 'btn.select' | 'btn.copy' | 'btn.copied'
    // Status
    | 'status.running' | 'status.offline' | 'status.installed'
    | 'status.notInstalled' | 'status.scanning' | 'status.paused'
    | 'status.complete' | 'status.failed'
    // Search
    | 'search.skills'
    // Model dialog
    | 'model.name' | 'model.apiKey' | 'model.modelId'
    | 'model.openaiUrl' | 'model.anthropicUrl' | 'model.proxyNode'
    | 'model.editConfig' | 'model.proxyTunnel' | 'model.specificProxy'
    | 'model.deleteTitle' | 'model.deleteConfirm'
    | 'model.selectToTest' | 'model.escCancel' | 'model.enterSave'
    // ModelCard labels
    | 'model.label' | 'model.source' | 'model.latency' | 'model.debugTesting'
    | 'model.cloud' | 'model.local' | 'model.tunnel'
    // Skills
    | 'skills.details' | 'skills.selectToView'
    | 'skills.author' | 'skills.category' | 'skills.description'
    | 'skills.noDescription' | 'skills.noSkillsInCategory'
    | 'skills.catAll' | 'skills.catDevelopment' | 'skills.catMarketing'
    | 'skills.catDesign' | 'skills.catResearch' | 'skills.catAIML' | 'skills.catFinance'
    | 'skills.catNews' | 'skills.catSearch' | 'skills.catTools' | 'skills.catHealth'
    | 'skills.catCoding' | 'skills.catGame' | 'skills.catCreative'
    | 'skills.catProductivity' | 'skills.catEducation' | 'skills.catLanguage'
    | 'skills.favorites' | 'skills.noMatch' | 'skills.loading'
    | 'skills.noModelTitle' | 'skills.noModelMsg' | 'skills.fixOk' | 'skills.fixFailed'
    | 'skills.keyword'
    | 'skills.translateTo' | 'skills.translating'
    | 'skills.fixing' | 'skills.fixContent'
    | 'skills.removeFavorite' | 'skills.addFavorite'
    // App Manager
    | 'agent.myLocalModel' | 'agent.selectTool' | 'agent.selectModelFor'
    | 'agent.installedSkillsFor' | 'agent.noSkills'
    | 'agent.noModelsTitle' | 'agent.noModelsHintPre' | 'agent.noModelsHintPost'
    | 'agent.applyAndLaunch' | 'agent.appliedVia'
    | 'agent.modelsTab' | 'agent.skillsTab'
    | 'agent.installViaMother'
    // Tool categories
    | 'toolCat.all' | 'toolCat.agentOS' | 'toolCat.ide' | 'toolCat.cli'
    | 'toolCat.autoTrading' | 'toolCat.game' | 'toolCat.utility'
    // ToolCard labels
    | 'tool.models' | 'tool.skills' | 'tool.skillsInstalled'
    | 'tool.app' | 'tool.config'
    // Local Server
    | 'server.selectModel' | 'server.context' | 'server.port' | 'server.runtime'
    | 'server.removeDirectories' | 'server.removeDirectoryConfirm'
    | 'server.compute' | 'server.stdout'
    | 'server.selectFromPanel' | 'server.awaitingInit' | 'server.selectConfigStart'
    | 'server.local' | 'server.store'
    | 'server.selectModelDir' | 'server.downloadFromStore'
    | 'server.gpuFull' | 'server.cpuOnly'
    | 'server.setupEngine' | 'server.downloading'
    // Debug
    | 'debug.console' | 'debug.gettingStarted' | 'debug.selectModelForAI' | 'debug.selectModelHint'
    | 'debug.sendLogsToAI' | 'debug.selectModelFirst'
    | 'debug.ready' | 'debug.analyzing' | 'debug.idle' | 'debug.errors'
    // Download / Model Store
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
    // Close behavior confirmation
    | 'close.title' | 'close.message' | 'close.minimize' | 'close.quit' | 'close.remember'
    // Developer invite hint
    | 'hint.devInvite'
    // Channels
    | 'channel.standby' | 'channel.linked' | 'channel.enterMessage' | 'channel.awaitingResponse'
    | 'channel.failed' | 'channel.connecting' | 'channel.connectedTo'
    | 'channel.connectionFailed' | 'channel.transmitting' | 'channel.noModels'
    | 'channel.motherFlow'
    // Mother Agent
    | 'mother.selectModel'
    | 'mother.awaitingInit' | 'mother.flowHint'
    | 'mother.hintInstall' | 'mother.hintInstallSkills' | 'mother.hintShowSpecs' | 'mother.hintTroubleshoot' | 'mother.hintUninstall'
    | 'mother.executing' | 'mother.callingTool' | 'mother.processing'
    | 'mother.enterMessage' | 'mother.noModels' | 'mother.noFavorites'
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
    | 'ssh.winStep1' | 'ssh.winStep2' | 'ssh.winStep3'
    | 'ssh.winUsername' | 'ssh.winPassword' | 'ssh.winIp'
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
    | 'common.inputting';

export type Translations = Record<TKey, string>;
