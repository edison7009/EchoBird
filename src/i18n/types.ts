// i18n type definitions (separate file to avoid circular imports)

// Translation key definitions
export type TKey =
    // App
    | 'app.name'
    // Navigation
    | 'nav.modelNexus' | 'nav.skillBrowser' | 'nav.appManager'
    | 'nav.localServer' | 'nav.motherAgent' | 'nav.logsDebug'
    // Page titles
    | 'page.modelNexus' | 'page.skillBrowser' | 'page.appManager'
    | 'page.localServer' | 'page.motherAgent' | 'page.logsDebug'
    // Settings
    | 'settings.title' | 'settings.version' | 'settings.language'
    | 'settings.logsDebug' | 'settings.updates'
    | 'settings.checkForUpdates' | 'settings.checking'
    | 'settings.latestVersion' | 'settings.checkFailed'
    // Buttons
    | 'btn.addModel' | 'btn.apply' | 'btn.scanAgain' | 'btn.refresh'
    | 'btn.save' | 'btn.cancel' | 'btn.delete' | 'btn.edit'
    | 'btn.install' | 'btn.uninstall' | 'btn.launchApp' | 'btn.loading'
    | 'btn.open' | 'btn.modifyOnly' | 'btn.start' | 'btn.stop'
    | 'btn.add' | 'btn.remove' | 'btn.saveModel' | 'btn.compute'
    | 'btn.sendLogs'
    // Status
    | 'status.running' | 'status.offline' | 'status.installed'
    | 'status.notInstalled' | 'status.scanning' | 'status.paused'
    // Search
    | 'search.skills'
    // Model dialog
    | 'model.name' | 'model.apiKey' | 'model.modelId'
    | 'model.openaiUrl' | 'model.anthropicUrl' | 'model.proxyNode'
    | 'model.editConfig' | 'model.proxyTunnel' | 'model.specificProxy'
    | 'model.deleteTitle' | 'model.deleteConfirm'
    | 'model.selectToTest' | 'model.escCancel' | 'model.enterSave'
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
    | 'agent.applyAndLaunch' | 'agent.appliedVia'
    // Local Server
    | 'server.selectModel' | 'server.context' | 'server.port' | 'server.runtime'
    | 'server.removeDirectories' | 'server.removeDirectoryConfirm'
    | 'server.compute' | 'server.stdout'
    | 'server.selectFromPanel' | 'server.awaitingInit' | 'server.selectConfigStart'
    | 'server.local' | 'server.store'
    | 'server.selectModelDir' | 'server.downloadFromStore'
    // Debug
    | 'debug.console' | 'debug.selectModelForAI' | 'debug.selectModelHint'
    | 'debug.sendLogsToAI' | 'debug.selectModelFirst'
    // Download / Model Store
    | 'download.location' | 'download.changePath' | 'download.selectNewDir'
    | 'quant.light' | 'quant.standard' | 'quant.extended' | 'quant.large' | 'quant.maximum'
    // Tool categories (Agent Worker page)
    | 'toolCat.all' | 'toolCat.agentOS' | 'toolCat.ide' | 'toolCat.cli'
    | 'toolCat.autoTrading' | 'toolCat.game' | 'toolCat.utility'
    // Agent Worker tabs
    | 'agent.modelsTab' | 'agent.skillsTab'
    // ToolCard labels
    | 'tool.models' | 'tool.skills' | 'tool.skillsInstalled'
    | 'tool.app' | 'tool.config'
    // Skills extra
    | 'skills.viewGithub' | 'skills.loading'
    // VRAM fitness labels
    | 'vram.easy' | 'vram.good' | 'vram.tight' | 'vram.heavy'
    // Download status
    | 'status.complete' | 'status.failed'
    | 'download.inQueue' | 'download.pause' | 'download.resume'
    | 'download.cancel' | 'download.retry'
    // Debug console
    | 'debug.ready' | 'debug.analyzing' | 'debug.idle' | 'debug.errors'
    // Local Server
    | 'server.gpuFull' | 'server.cpuOnly'
    | 'server.setupEngine' | 'server.downloading' | 'server.installing'
    // ModelStore buttons
    | 'store.add' | 'store.del' | 'store.cancel' | 'store.remove'
    | 'store.ver' | 'store.ready'
    // ModelCard labels
    | 'model.label' | 'model.source' | 'model.latency' | 'model.debugTesting'
    | 'model.cloud' | 'model.local' | 'model.tunnel'
    // Copy button
    | 'btn.copy' | 'btn.copied'
    // Common
    | 'common.noData' | 'common.confirm' | 'common.website'
    | 'common.areYouSure'
    // Close behavior confirmation
    | 'close.title' | 'close.message' | 'close.minimize' | 'close.quit' | 'close.remember'
    // Close behavior settings
    | 'settings.closeBehavior' | 'settings.closeAsk' | 'settings.closeMinimize' | 'settings.closeQuit'
    | 'btn.select'
    // API Key encryption status
    | 'key.encrypted' | 'key.destroyed'
    // Main Console
    | 'nav.channels' | 'page.channels'
    // Channels
    | 'channel.standby' | 'channel.linked' | 'channel.enterMessage' | 'channel.awaitingResponse'
    | 'channel.remoteLlm' | 'channel.llmPanel' | 'channel.deployFirst'
    | 'channel.failed' | 'channel.connecting' | 'channel.connectedTo'
    | 'channel.connectionFailed' | 'channel.transmitting' | 'channel.noModels'
    | 'channel.motherFlow'
    // Developer invite hint
    | 'hint.devInvite'
    // Mother Agent
    | 'mother.selectModel'
    | 'mother.deployHint' | 'mother.awaitingInit' | 'mother.flowHint'
    | 'mother.hintInstallOC' | 'mother.hintInstallSkills' | 'mother.hintDeployLlm' | 'mother.hintShowSpecs' | 'mother.hintUninstallOC'
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
    | 'common.showProcess'
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
    | 'ssh.ishUsername' | 'ssh.ishPassword' | 'ssh.ishIp';

export type Translations = Record<TKey, string>;
