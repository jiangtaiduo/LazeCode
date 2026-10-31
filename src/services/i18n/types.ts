// 多语言字典的类型契约。
//
// 设计约束（来自项目现状，非通用 i18n 库）：
// 1. **编译期对齐**：两语言用同一个 `LocaleDict` 约束，少一个 key 就 tsc 报错。
//    运行时才发现缺 key 的方案在此项目不可接受 —— 680 个 key 无人校对。
// 2. **不用 ICU MessageFormat**：本项目无复数/性别/选择规则，引入它是纯负担。
//    需要插值时字典项是 `(args) => string`，模板在各 locale 内部自行拼接。
// 3. **英文是基准**：`en` 必须字面齐全，`zhCN` 用 `satisfies LocaleDict` 对齐到它。
//    缺 key 的兜底在前端查表处回落 en（见 index.ts），不依赖浏览器 Intl。
// 4. 新增语言 = 加一个 `const xx: LocaleDict` + 在 `SUPPORTED_LOCALES` 注册，
//    不必触碰任何调用点。

/** 支持的语言。key 是设置里持久化的值（Bcp47 简化式）。 */
export type Language = 'en' | 'zh-CN';

/** 插值型文案：参数顺序由 locale 自己决定，英文与中文可以不同。 */
export type LocaleText = string;
/** 带占位符的文案。用函数而非字符串，避免不同语言语序差异破坏拼接。 */
export type LocaleFn<A extends unknown[] = []> = (...args: A) => string;

export interface LocaleDict {
  /** 原生应用菜单（macOS 屏幕顶部 / Windows 窗口内）。由 Rust 侧按 label 提取。 */
  menu: {
    // 注意：没有 `app` 字段。macOS 首个子菜单（应用菜单）的标题是产品名
    // "LazeCode"，不是可翻译文案 —— Rust 侧用 `menu::APP_NAME`。
    // 「关于 LazeCode」的 about 在下面。
    file: string;
    edit: string;
    view: string;
    navigate: string;
    git: string;
    help: string;
    newWindow: string;
    openProject: string;
    closeProject: string;
    save: string;
    saveAll: string;
    closeTab: string;
    closeAllTabs: string;
    undo: string;
    redo: string;
    cut: string;
    copy: string;
    paste: string;
    selectAll: string;
    toolWindows: string;
    project: string;
    commit: string;
    gitLog: string;
    terminal: string;
    preferences: string;
    searchEverywhere: string;
    goToLine: string;
    commitEllipsis: string;
    push: string;
    updateProject: string;
    fetch: string;
    manageRemotes: string;
    showHistory: string;
    stash: string;
    unstash: string;
    shelve: string;
    resolveConflicts: string;
    refresh: string;
    quit: string;
    about: string;
    checkForUpdates: string;
  };
  /** 状态栏 / 通知类一次性消息（showStatus 的实参）。 */
  status: {
    saved: string;
    saveFailed: LocaleFn<[string]>;
    saveAllDone: LocaleFn<[number]>;
    /** 项目快照超过上限但有终端被 pin 住（参数为超出数量） */
    projectsPinnedWarning: LocaleFn<[number]>;
    openProjectFailedAuth: LocaleFn<[string]>;
    loadWorkspaceFailed: LocaleFn<[string]>;
    /** 项目层设置（.lazecode/settings.json）有警告：参数为 warnings.join('; ') */
    projectSettingsWarnings: LocaleFn<[string]>;
    /** 项目层设置损坏：参数为解析错误 */
    projectSettingsBroken: LocaleFn<[string]>;
    openDirFailed: LocaleFn<[string]>;
    dirAuthFailed: LocaleFn<[string]>;
    readDirFailed: LocaleFn<[string]>;
    dirtyAutoRecovered: LocaleFn<[string, string]>;
    externalDeletedDirty: LocaleFn<[string]>;
    externalChangedKeepLocal: string;
    replaceSkippedDirty: LocaleFn<[string]>;
    diskReadFailedKeepLocal: string;
    fileRecoverFailed: LocaleFn<[string, string]>;
    clipboardEmpty: string;
    copyFailed: LocaleFn<[string]>;
    copied: LocaleFn<[number]>;
    copiedFiles: LocaleFn<[number]>;
    notGitRepo: string;
    languageChanged: LocaleFn<[string]>;
    /** 外部删除但有未保存修改（标签页保留） */
    externalDeletedKeepTab: LocaleFn<[string]>;
    /** 外部删除且干净（标签页关闭） */
    externalDeletedTabClosed: LocaleFn<[string]>;
    reloadFailedNoDisk: LocaleFn<[string]>;
    openFileFailed: LocaleFn<[string]>;
    savedFile: LocaleFn<[string]>;
    stillDirtyCancelledCount: LocaleFn<[number]>;
    exitSaveBlocked: LocaleFn<[number]>;
    /** 浏览器预览（非桌面端）不提供原生选择器 */
    browserPreviewNoDirPicker: string;
    browserPreviewNoFilePicker: string;
    dirAlreadyRoot: string;
    dirAlreadyInTree: string;
    removedFromTree: LocaleFn<[string]>;
    gitDataReadFailed: LocaleFn<[string]>;
    gitRepoInitialized: string;
    gitInitFailed: LocaleFn<[string]>;
    deleteFailedList: LocaleFn<[string]>;
    cut: LocaleFn<[string]>;
    copiedOne: LocaleFn<[string]>;
    pastedTo: LocaleFn<[string, string]>;
    pasteFailed: LocaleFn<[string]>;
    pathCopied: LocaleFn<[string]>;
    copyPathFailed: string;
    showInFileManagerFailed: LocaleFn<[string]>;
    openedInExternalTerminal: LocaleFn<[string]>;
    openTerminalFailed: LocaleFn<[string]>;
    reloadedFromDisk: LocaleFn<[string]>;
    expandDirFailed: LocaleFn<[string]>;
    createdFile: LocaleFn<[string]>;
    createdDir: LocaleFn<[string]>;
    renamedTo: LocaleFn<[string]>;
    fileOpFailed: LocaleFn<[string]>;
    pickFileFailed: LocaleFn<[string]>;
    projectClosedBackTo: LocaleFn<[string]>;
    projectClosedNoneLeft: string;
  };
  /** 原生模态（window.confirm / prompt / alert）的标题与正文。 */
  dialog: {
    unsavedOnOpen: LocaleFn<[number]>;
    unsavedOnOpenTitle: string;
    stillDirtyCancelled: LocaleFn<[number]>;
    branchDeleteConfirm: LocaleFn<[string]>;
    forceCheckoutConfirm: LocaleFn<[string]>;
    deleteFileConfirm: LocaleFn<[string]>;
    deleteDirConfirm: LocaleFn<[string, string]>;
    ok: string;
    cancel: string;
    discard: string;
    save: string;
    // ---- 破坏性 Git 操作（标题 + 正文）----
    deleteRemoteBranchTitle: string;
    deleteRemoteBranchConfirm: LocaleFn<[string]>;
    deleteBranchTitle: string;
    deleteBranchConfirm: LocaleFn<[string]>;
    deleteRefsConfirm: LocaleFn<[string]>;
    deleteRefsTitle: string;
    refsDeleted: LocaleFn<[number]>;
    refsDeleteFailed: LocaleFn<[string]>;
    checkoutAndRebaseConfirm: LocaleFn<[string, string]>;
    checkoutAndRebaseTitle: string;
    forceDeleteBranchTitle: string;
    forceDeleteBranchConfirm: LocaleFn<[string]>;
    checkoutCommitTitle: string;
    checkoutCommitConfirm: LocaleFn<[string]>;
    undoCommitTitle: string;
    undoCommitConfirm: LocaleFn<[string]>;
    mergeBranchTitle: string;
    mergeBranchConfirm: LocaleFn<[string, string]>;
    rebaseBranchTitle: string;
    rebaseBranchConfirm: LocaleFn<[string, string]>;
    dropCommitTitle: string;
    dropCommitConfirm: LocaleFn<[string]>;
    dropCommitDirtyTitle: string;
    dropCommitDirtyConfirm: LocaleFn<[string]>;
    dropCommitDirtyOk: string;
    dropCommitsTitle: string;
    dropCommitsOk: string;
    dropCommitsConfirm: LocaleFn<[number]>;
    checkoutTagTitle: string;
    checkoutTagConfirm: LocaleFn<[string]>;
    checkoutTagOk: string;
    deleteTagTitle: string;
    deleteTagConfirm: LocaleFn<[string]>;
    deleteTagOk: string;
    checkoutRefTitle: string;
    checkoutRefConfirm: LocaleFn<[string]>;
    checkoutRefOk: string;
    overwriteWorktreeTitle: string;
    overwriteWorktreeConfirm: LocaleFn<[string, string]>;
    rollbackFileTitle: string;
    rollbackFileConfirm: LocaleFn<[string]>;
    rollbackDirTitle: string;
    rollbackDirConfirm: LocaleFn<[string]>;
    revertCommitTitle: string;
    revertCommitConfirm: LocaleFn<[string]>;
    trashUnversionedTitle: string;
    trashUnversionedConfirm: LocaleFn<[number]>;
    unsavedOnClose: LocaleFn<[number]>;
    // ---- 终端关闭 ----
    consoleRunningTitle: string;
    consoleRunningConfirm: LocaleFn<[string]>;
    consoleRunningOk: string;
    // ---- 贮藏 / 贮藏架 ----
    deleteStashTitle: string;
    deleteStashConfirm: LocaleFn<[string]>;
    deleteShelfTitle: string;
    deleteShelfConfirm: LocaleFn<[string]>;
    // ---- 交互式变基 ----
    interactiveRebaseTitle: string;
    interactiveRebaseConfirm: LocaleFn<[string, number]>;
  };
  /** 差异查看器（DiffViewer）：工具栏、视图模式、空态、行尾换行提示 */
  diff: {
    previousHunk: string;
    nextHunk: string;
    previousFile: string;
    nextFile: string;
    unifiedView: string;
    sideBySideView: string;
    stats: LocaleFn<[number, number, number]>;
    /** 边界提示：已到达首/末处差异，且列表里还有别的文件 */
    edgeFirstFile: string;
    edgeLastFile: string;
    edgeNextFile: string;
    edgePrevFile: string;
    edgeFirst: string;
    edgeLast: string;
    edgeFirstFileFirst: string;
    edgeLastFileLast: string;
    /** 合并行（-/+ 内容相同）的 tooltip */
    mergedEofTitle: string;
    mergedNewlineTitle: string;
    eofTailNote: string;
    newlineOnlyNote: string;
    /** 普通行尾无换行标记的 tooltip */
    delEofTitle: string;
    newEofTitle: string;
    lineNoNewlineTitle: string;
    /** 标题栏比较范围的兜底标签（非提交、非 ref） */
    workingTree: string;
    addedTag: string;
    deletedTag: string;
    /** 超大 diff 被行数封顶时的标题栏标记（计数仍是全量的，见 truncatedHint） */
    truncatedTag: string;
    truncatedHint: string;
    loadingEditor: string;
    loadingDiff: string;
    readFailed: LocaleFn<[string]>;
    /** 有 diff 数据但没有任何可显示文本行时的空态 */
    noTextDiffInCommit: string;
    binaryNoText: string;
    noDiffStagedHead: string;
    noUncommittedChanges: string;
    cmpWorktree: LocaleFn<[string]>;
    cmpCommit: LocaleFn<[string]>;
    cmpCommits: LocaleFn<[number]>;
    cmpCommitRange: LocaleFn<[string, string]>;
    cmpStagedHead: string;
    cmpWorktreeIndex: string;
    viewStaged: string;
    viewUnstaged: string;
    stageHunk: string;
    unstageHunk: string;
    rollbackHunk: string;
    hunkActionFailed: LocaleFn<[string]>;
  };
  /** 编辑器（EditorView）：git 工具栏、图片缩放、错误面板、blame tooltip */
  editor: {
    showHistory: string;
    history: string;
    annotateBlame: string;
    zoomOut: string;
    zoomIn: string;
    zoomReset: string;
    cannotOpenFile: string;
    blameTooltip: LocaleFn<[string, string, string]>;
    revealInFinder: string;
    forceOpen: string;
    /** 大文件分页模式（只读，不进 Monaco）：横幅与翻页器文案 */
    pagedBanner: LocaleFn<[string, string, string]>;
    pagedPrev: string;
    pagedNext: string;
    pagedGo: string;
    pagedOpenFull: string;
    pagedPageOf: LocaleFn<[string, string]>;
  };
  /** 项目抽屉（ProjectDrawer）：工具栏、空态、外部目录徽标 */
  drawer: {
    projectTitle: string;
    locateInTree: string;
    autoLocate: string;
    collapseAll: string;
    openExternalDir: string;
    noProjectOpen: string;
    openFolder: string;
    externalDirTooltip: LocaleFn<[string]>;
    externalBadge: string;
    removeExternalDir: string;
    nodeContextHint: string;
    containsUncommitted: string;
  };
  /** 全局搜索（Search Everywhere） */
  search: {
    tabFiles: string;
    tabContent: string;
    tabActions: string;
    tabHint: string;
    placeholderFiles: string;
    placeholderContent: string;
    searchFailed: LocaleFn<[string]>;
    searchingFiles: string;
    noFileMatch: string;
    noOpenFiles: string;
    searchingContent: string;
    noContentMatch: string;
    enterKeyword: string;
    navUpDown: string;
    openEnter: string;
    escExit: string;
  };
  /** Find in Files / Replace in Files */
  findInFiles: {
    title: string;
    placeholderSearch: string;
    placeholderReplace: string;
    showReplace: string;
    caseSensitive: string;
    wholeWord: string;
    regex: string;
    includePlaceholder: string;
    scope: string;
    allProject: string;
    resultsCount: LocaleFn<[number]>;
    noResults: string;
    searching: string;
    replaceAll: string;
    replaceConfirm: LocaleFn<[number, number]>;
    replaced: LocaleFn<[number, number]>;
    replaceFailed: LocaleFn<[string]>;
    openFileHint: string;
    close: string;
  };
  /** 合并冲突解决对话框 */
  conflict: {
    title: string;
    remaining: LocaleFn<[number]>;
    /** 副标题：操作上下文（对齐 PyCharm "Conflicts during cherry-picking commit …"） */
    subtitleMerge: LocaleFn<[string, string]>;
    subtitleMergeBare: string;
    subtitleCherryPick: LocaleFn<[string, string]>;
    subtitleCherryPickBare: LocaleFn<[string]>;
    subtitleGeneric: string;
    subtitleRebase: string;
    /** Yours/Theirs 列头（含分支/提交上下文，CSS 省略超长） */
    colName: string;
    colYours: string;
    colTheirs: string;
    yoursCtx: LocaleFn<[string]>;
    theirsCtx: LocaleFn<[string]>;
    statusDeleted: string;
    statusModified: string;
    groupByDirectory: string;
    selectHint: string;
    acceptYours: string;
    acceptTheirs: string;
    mergeDots: string;
    allResolved: string;
    usedSide: LocaleFn<[string, string]>;
    resolved: LocaleFn<[string]>;
    deletedByUs: LocaleFn<[string]>;
    deletedByThem: LocaleFn<[string]>;
    deleteHint: string;
    alreadyResolved: LocaleFn<[string]>;
    /** 三路合并工具（Merge Revisions）：逐块 ≫/≪ + Apply/Cancel */
    mergeTool: {
      title: LocaleFn<[string]>;
      conflicts: LocaleFn<[number]>;
      noConflicts: string;
      prevTitle: string;
      nextTitle: string;
      takeLeftTitle: string;
      takeRightTitle: string;
      takeKeepTitle: string;
      acceptLeft: string;
      acceptRight: string;
      cancel: string;
      apply: string;
      stillMarked: LocaleFn<[number]>;
      applied: LocaleFn<[string]>;
      /** 状态行（PyCharm: "2 changes. No conflicts."） */
      statusChanges: LocaleFn<[number]>;
      statusNoChanges: string;
      statusConflicts: LocaleFn<[number]>;
      statusNoConflicts: string;
      localPane: string;
      resultPane: string;
      remotePane: LocaleFn<[string]>;
      /** 右窗格标题：cherry-pick 现场带操作词（PyCharm: "Changes from cherry-pick a9d33159"） */
      remotePaneCherry: LocaleFn<[string]>;
      /** 侧栏头 Show Details 链接 */
      showDetails: string;
      /** 工具栏 "Apply non-conflicting changes:" 静态标签 */
      applyLabel: string;
      applyAllTitle: string;
      applyFromLeftTitle: string;
      applyFromRightTitle: string;
      /** 魔法棒：一键双侧应用（对齐 PyCharm 的 ✎⚡ 位） */
      wandTitle: string;
      /** ⇅：自动应用非冲突更改的开关 */
      autoApplyTitle: string;
      gearTitle: string;
      /** 空白忽略下拉（PyCharm 官方文案五档） */
      wsNone: string;
      wsTrim: string;
      wsSpace: string;
      wsSpaceEmpty: string;
      wsImports: string;
      /** 高亮模式下拉（PyCharm 官方文案五档） */
      hlWords: string;
      hlLines: string;
      hlSplit: string;
      hlChars: string;
      hlNone: string;
      /** ⚙ 菜单项 */
      gearAutoApply: string;
      /** 侧栏 gutter 标记 tooltip */
      sideApplyTitle: string;
      sideIgnoreTitle: string;
    };
  };
  /** Push 对话框 */
  push: {
    readingPending: string;
    pendingCount: LocaleFn<[number]>;
    noUpstreamNote: string;
    allPushed: string;
    forceLabel: string;
    tagsLabel: string;
    pushingTo: LocaleFn<[string, number]>;
    waitingHint: LocaleFn<[number]>;
    pushing: string;
    manageRemotes: string;
    remote: string;
    cancel: string;
    pushButton: string;
    title: LocaleFn<[string, string]>;
  };
  /** Update Project（Pull）对话框 */
  pull: {
    title: LocaleFn<[string]>;
    mergeDesc: string;
    mergeHint: string;
    rebaseDesc: string;
    rebaseHint: string;
    updating: string;
  };
  /** Stash Changes 对话框 */
  stash: {
    title: string;
    trackedCount: LocaleFn<[number]>;
    untrackedCount: LocaleFn<[number]>;
    cleanTree: string;
    messageLabel: string;
    messagePlaceholder: string;
    includeUntracked: string;
    noLocalChanges: string;
    stashedDefault: string;
    stashed: LocaleFn<[string]>;
    stashing: string;
  };
  /** Reset Current Branch to Here 对话框 */
  reset: {
    softDesc: string;
    mixedDesc: string;
    hardDesc: string;
    keepDesc: string;
    noCommitMessage: string;
    hardWarning: string;
    resetting: string;
  };
  /** Unstash Changes 对话框 */
  unstash: {
    title: string;
    empty: string;
    basedOn: LocaleFn<[string]>;
    fileCount: LocaleFn<[number]>;
    noFileChanges: string;
    selectStash: string;
    dropped: LocaleFn<[string]>;
    popped: LocaleFn<[string]>;
    applied: LocaleFn<[string]>;
    conflictHint: LocaleFn<[string]>;
    dropTooltip: string;
    applyTooltip: string;
    popTooltip: string;
  };
  /** Shelve／Unshelve 对话框 */
  shelve: {
    title: string;
    emptyHint: string;
    unshelvedKeep: LocaleFn<[string]>;
    unshelvedDeleted: LocaleFn<[string]>;
    applyFailed: LocaleFn<[string]>;
    unshelveKeepLabel: string;
    unshelveKeepTooltip: string;
    unshelveDeleteTooltip: string;
  };
  /** 交互式变基对话框 */
  rebase: {
    hint: string;
    noCommits: string;
    moveUp: string;
    moveDown: string;
    done: LocaleFn<[number]>;
    conflictHint: LocaleFn<[string]>;
    running: string;
  };
  /** 压缩提交（Squash Commits）对话框 */
  squashDialog: {
    title: string;
    summary: LocaleFn<[number]>;
    messageLabel: string;
    squashBtn: string;
    cancelBtn: string;
  };
  /** 文件历史（Show History for File）对话框 */
  fileHistory: {
    closeEsc: string;
    commitMode: string;
    commitModeTooltip: string;
    worktreeMode: string;
    worktreeModeTooltip: string;
    noHistory: string;
    selectCommit: string;
  };
  /** 远程管理（Git Remotes，对齐 PyCharm Git → Manage Remotes…） */
  clone: {
    title: string;
    urlLabel: string;
    urlPlaceholder: string;
    destinationLabel: string;
    destinationPlaceholder: string;
    browse: string;
    cloneButton: string;
    cloning: string;
    failed: LocaleFn<[string]>;
  };
  gitTools: {
    title: string;
    submodules: string;
    worktrees: string;
    lfs: string;
    credentials: string;
    refresh: string;
    close: string;
    add: string;
    update: string;
    sync: string;
    remove: string;
    path: string;
    url: string;
    branch: string;
    status: string;
    empty: string;
    lfsUnavailable: string;
    lfsVersion: LocaleFn<[string]>;
    lfsFiles: LocaleFn<[number]>;
    pull: string;
    track: string;
    trackPlaceholder: string;
    helperLabel: string;
    helperPlaceholder: string;
    saveHelper: string;
    saved: string;
    keychainAvailable: string;
    keychainMissing: string;
    addSubmodule: string;
    addWorktree: string;
    createBranch: string;
    working: string;
    done: LocaleFn<[string]>;
    error: LocaleFn<[string]>;
  };
  remotes: {
    title: string;
    add: string;
    edit: string;
    remove: string;
    close: string;
    save: string;
    cancel: string;
    empty: string;
    nameLabel: string;
    urlLabel: string;
    namePlaceholder: string;
    urlPlaceholder: string;
    defineNew: string;
    defineEdit: LocaleFn<[string]>;
    removeTitle: string;
    removeConfirm: LocaleFn<[string]>;
  };
  /** 项目树「新建 / 重命名 / 删除确认」对话框 */
  namePrompt: {
    titleNewFile: string;
    titleNewDir: string;
    titleRename: string;
    titleDelete: string;
    batchDelete: LocaleFn<[number]>;
    deleteFileMsg: LocaleFn<[string]>;
    deleteDirMsg: LocaleFn<[string]>;
    fileNamePlaceholder: string;
    dirNamePlaceholder: string;
    newNamePlaceholder: string;
    cancel: string;
    delete: string;
    rename: string;
    create: string;
  };
  /** 分支比较（Compare / Show Diff with Working Tree）对话框 */
  compare: {
    worktreeTitle: LocaleFn<[string]>;
    uniqueCommits: LocaleFn<[number, string]>;
    noUniqueCommits: string;
    diffFileCount: LocaleFn<[number]>;
    noFileDiff: string;
  };
  /** Compare 抽屉（两提交/分支对比）。 */
  compareDrawer: {
    changesBetween: LocaleFn<[string, string]>;
    close: string;
    moreOptions: string;
    minimize: string;
    swapRevisions: string;
    viewOptions: string;
    groupBy: string;
    directory: string;
    expandAll: string;
    collapseAll: string;
    loading: string;
    noDifferences: string;
  };
  /** Compare Branches：两条独立提交日志 + 各自提交详情（不是 Compare Versions）。 */
  compareBranches: {
    tabTitle: LocaleFn<[string, string]>;
    commitsOnlyIn: LocaleFn<[string, string]>;
    swap: string;
    loading: string;
    loadFailed: string;
    retry: string;
    noCommits: LocaleFn<[string]>;
    noMatches: string;
    noSelection: string;
    noFiles: string;
    fileCount: LocaleFn<[number]>;
    truncated: LocaleFn<[number]>;
    allSelected: LocaleFn<[number]>;
    inBranches: LocaleFn<[number, string]>;
    filterRef: string;
    allRefs: string;
    openDiff: string;
    expandAll: string;
    collapseAll: string;
    detailsHint: string;
    resizeSplit: string;
    resizeDetails: string;
  };
  /** Git 面板的 toast / 输入框 / 操作反馈（showGitToast + nativePrompt）。 */
  git: {
    // ---- 分支 ----
    branchCreatePrompt: LocaleFn<[string]>;
    branchRenamePrompt: LocaleFn<[string]>;
    branchCreatedFrom: LocaleFn<[string, string]>;
    branchRenamed: LocaleFn<[string, string]>;
    branchDeleted: LocaleFn<[string]>;
    branchForceDeleted: LocaleFn<[string]>;
    branchDeleteFailed: LocaleFn<[string]>;
    branchSwitchFailed: LocaleFn<[string]>;
    branchCreateFailed: LocaleFn<[string]>;
    branchRenameFailed: LocaleFn<[string]>;
    // ---- 提交 ----
    commitMessageEmpty: string;
    committed: LocaleFn<[string]>;
    commitFailed: LocaleFn<[string]>;
    amendMessagePrompt: string;
    amendDone: LocaleFn<[string]>;
    amendFailed: LocaleFn<[string]>;
    commitDropped: LocaleFn<[string]>;
    squashDone: LocaleFn<[number]>;
    squashFailed: LocaleFn<[string]>;
    hashCopied: LocaleFn<[string]>;
    copyFailed: string;
    detachedPushBlocked: string;
    // ---- 推送 / 拉取 ----
    pushSucceeded: LocaleFn<[string]>;
    pushNothing: string;
    updateDone: LocaleFn<[string]>;
    updateDoneRebase: string;
    updateDoneMerge: string;
    updateFailed: LocaleFn<[string]>;
    fetchDone: string;
    fetchFailed: LocaleFn<[string]>;
    // ---- 摘取 / 合并 / 变基 ----
    cherryPicked: LocaleFn<[string]>;
    cherryPickManyDone: LocaleFn<[number]>;
    cherryPickConflicted: LocaleFn<[string]>;
    cherryPickFailed: LocaleFn<[string]>;
    conflictsResolved: LocaleFn<[string]>;
    merged: LocaleFn<[string, string]>;
    rebased: LocaleFn<[string, string]>;
    mergeFailed: LocaleFn<[string]>;
    rebaseFailed: LocaleFn<[string]>;
    // ---- 标签 ----
    tagCreatePrompt: LocaleFn<[string]>;
    tagMessagePrompt: string;
    tagCreated: LocaleFn<[string]>;
    tagCreateFailed: LocaleFn<[string]>;
    // ---- 检出 / 回滚 ----
    checkoutFailed: LocaleFn<[string]>;
    checkedOut: LocaleFn<[string]>;
    checkedOutTag: LocaleFn<[string]>;
    checkedOutRef: LocaleFn<[string]>;
    checkoutRefPrompt: string;
    tagDeleted: LocaleFn<[string]>;
    tagDeleteFailed: LocaleFn<[string]>;
    fileReverted: LocaleFn<[string]>;
    fileRevertFailed: LocaleFn<[string]>;
    dirReverted: LocaleFn<[string]>;
    revertedCommit: LocaleFn<[string]>;
    revertManyDone: LocaleFn<[number]>;
    commitsDropped: LocaleFn<[number]>;
    revertFailed: LocaleFn<[string]>;
    // ---- 文件操作 ----
    filesAdded: LocaleFn<[string]>;
    filesAddFailed: LocaleFn<[string]>;
    onlyCommitFilePrompt: LocaleFn<[string]>;
    onlyCommitFilesPrompt: LocaleFn<[number]>;
    goToLinePrompt: string;
    // ---- 变更列表 ----
    changelistCreated: LocaleFn<[string]>;
    changelistCreateFailed: LocaleFn<[string]>;
    changelistDeletedRollback: LocaleFn<[string]>;
    changelistDeletedToChanges: LocaleFn<[string]>;
    changelistDeleteFailed: LocaleFn<[string]>;
    changelistMovedMany: LocaleFn<[string, number]>;
    changelistMovedOne: LocaleFn<[string, string]>;
    changelistMoveFailed: LocaleFn<[string]>;
    // ---- 重置 / 贮藏 ----
    branchCreatedPrompt: LocaleFn<[string]>;
    resetDone: LocaleFn<[string, string, string]>;
    resetFailed: LocaleFn<[string]>;
    undoCommitDone: LocaleFn<[string]>;
    undoCommitFailed: LocaleFn<[string]>;
    stashNamePrompt: string;
    stashNoChanges: string;
    stashDone: LocaleFn<[string]>;
    stashFailed: LocaleFn<[string]>;
    unstashDone: LocaleFn<[string, string, number]>;
    unstashFailed: LocaleFn<[string]>;
    // ---- 补丁 / 剪贴板 ----
    patchSaved: LocaleFn<[string]>;
    patchSaveFailed: LocaleFn<[string]>;
    patchGenerateFailed: LocaleFn<[string]>;
    copiedToClipboard: LocaleFn<[string]>;
    clipboardUnavailable: string;
    browserPreviewNoSave: string;
    // ---- 工作区会话 ----
    autoRecoverFailed: LocaleFn<[string, string]>;
    releaseSessionFailed: LocaleFn<[string, string]>;
    autoSavedFiles: LocaleFn<[string]>;
    // ---- 原生目录/文件对话框标题 ----
    openWorkspaceDirTitle: string;
    openExternalDirTitle: string;
    gitDataRefreshed: LocaleFn<[string]>;
    // ---- 冲突 / 失败（含后续命令提示的多行消息）----
    cherryPickConflictHint: LocaleFn<[string]>;
    mergeConflictHint: LocaleFn<[string]>;
    rebaseConflictHint: LocaleFn<[string]>;
    rebaseAbortHint: LocaleFn<[string]>;
    dropCommitConflictHint: LocaleFn<[string]>;
    dropCommitFailed: LocaleFn<[string]>;
    // ---- 补丁复制标签 ----
    commitPatchLabel: LocaleFn<[string]>;
    localPatchLabel: LocaleFn<[string]>;
    // ---- 删除到废纸篓 ----
    deletedManyToTrash: LocaleFn<[number]>;
    deletedOneToTrash: LocaleFn<[string]>;
    // ---- 原生文件选择器标题 ----
    pickCompareFileTitle: LocaleFn<[string]>;
    // ---- 状态栏 ----
    dirtyFilesHint: LocaleFn<[number]>;
    ready: string;
    tabCount: LocaleFn<[number]>;
    spacesCount: LocaleFn<[number]>;
    indentFollowsFile: string;
    loadingEditor: string;
    loadingMarkdown: string;
    dragResizeCommitWidth: string;
    dragResizeTreeWidth: string;
    dragResizePanelHeight: string;
    dragResizeConsoleHeight: string;
    dragResizeConsoleWidth: string;
  };
  /** 后端错误码 -> 文案。码由 Rust `error_proto.rs` 产生，见 errors.ts 的协议说明。 */
  backendErrors: {
    // ---- 文件系统 / 工作区 ----
    pathNotFound: LocaleFn<[string]>;
    dirNotFound: LocaleFn<[string]>;
    srcNotFound: LocaleFn<[string]>;
    destDirNotFound: LocaleFn<[string]>;
    notADirectory: LocaleFn<[string]>;
    notADir: LocaleFn<[string]>;
    ioError: LocaleFn<[string]>;
    notAFile: LocaleFn<[string]>;
    nameEmpty: string;
    nameInvalid: LocaleFn<[string]>;
    pathOutsideWorkspace: LocaleFn<[string]>;
    workspaceReadOnly: LocaleFn<[string]>;
    workspaceMissing: LocaleFn<[string]>;
    credentialHelperRejected: LocaleFn<[string]>;
    workspaceRegistryLocked: string;
    // ---- git ----
    refUnsupported: LocaleFn<[string]>;
    stashNotFound: LocaleFn<[string]>;
    stashNameInvalid: string;
    changelistNotFound: LocaleFn<[string]>;
    conflictNotFound: LocaleFn<[string]>;
    // ---- pty / 终端 ----
    ptyOpenFailed: LocaleFn<[string]>;
    ptySpawnFailed: LocaleFn<[string]>;
    terminalCreateFailed: LocaleFn<[string]>;
    shellUnsupported: LocaleFn<[string]>;
    shellNameInvalid: LocaleFn<[string]>;
    // ---- 通用 ----
    // ---- 第二批（文件/目录/git 细节）----
    refInvalidLeadingDash: LocaleFn<[string]>;
    pathUnresolvable: LocaleFn<[string]>;
    createTargetFailed: LocaleFn<[string]>;
    noAvailableTargetName: LocaleFn<[string]>;
    symlinkCopyUnsupported: LocaleFn<[string]>;
    targetExistsNoOverwrite: LocaleFn<[string]>;
    unsupportedEntryType: LocaleFn<[string]>;
    fileAlreadyExists: LocaleFn<[string]>;
    dirAlreadyExists: LocaleFn<[string]>;
    alreadyExists: LocaleFn<[string]>;
    commitHashInvalid: LocaleFn<[string]>;
    stashNameOutOfRange: LocaleFn<[string]>;
    deleteFailed: LocaleFn<[string]>;
    changelistAlreadyExists: LocaleFn<[string]>;
    unknownMergeSide: LocaleFn<[string]>;
    mergeTooLarge: LocaleFn<[string]>;
    pathOutsideRepo: LocaleFn<[string]>;
    absolutePathRejected: LocaleFn<[string]>;
    dotdotPathRejected: LocaleFn<[string]>;
    resetModeUnsupported: LocaleFn<[string]>;
    rebaseTodoMissingRev: LocaleFn<[string]>;
    rebaseTodoBadRev: LocaleFn<[string]>;
    // ---- 终端 FFI（vt.rs / pty.rs：引擎调用失败，detail 是 rc 或上下文）----
    ffiTerminalNewFailed: LocaleFn<[string]>;
    ffiTerminalResizeFailed: LocaleFn<[string]>;
    ffiSetSelectionFailed: LocaleFn<[string]>;
    ffiClearSelectionFailed: LocaleFn<[string]>;
    ffiWordSelectFailed: LocaleFn<[string]>;
    ffiLineSelectFailed: LocaleFn<[string]>;
    ffiReadSelectionFailed: LocaleFn<[string]>;
    ffiSearchCreateFailed: LocaleFn<[string]>;
    ffiSearchSetTermFailed: LocaleFn<[string]>;
    ffiSearchRunFailed: LocaleFn<[string]>;
    ffiEncoderCreateFailed: LocaleFn<[string]>;
    ffiEventCreateFailed: LocaleFn<[string]>;
    ffiRowIterCreateFailed: LocaleFn<[string]>;
    ffiRowIterBindFailed: LocaleFn<[string]>;
    ffiRowViewFailed: LocaleFn<[string]>;
    ffiCellReadFailed: LocaleFn<[string]>;
    ffiSetColorFailed: LocaleFn<[string]>;
    searchQueryEmpty: string;
    searchGlobInvalid: LocaleFn<[string]>;
    searchRegexInvalid: LocaleFn<[string]>;
    worktreeDirty: LocaleFn<[string]>;
    mergeCommitDrop: LocaleFn<[string]>;
    commitPublished: LocaleFn<[string]>;
    commitNotInBranch: string;
    commitMessageEmpty: string;
    patchEmpty: string;
    patchTooLarge: LocaleFn<[string]>;
    encodingUnsupported: LocaleFn<[string]>;
    encodingLossy: LocaleFn<[string]>;
    encodeOverflow: LocaleFn<[string]>;
    ptyReaderFailed: LocaleFn<[string]>;
    ptyWriterFailed: LocaleFn<[string]>;
    // ---- 外部链接 / 大文件 / 图片 ----
    externalUrlRejected: LocaleFn<[string]>;
    fileTooLarge: LocaleFn<[string]>;
    dotgitPathRejected: LocaleFn<[string]>;
    pathspecMagicRejected: LocaleFn<[string]>;
    treeTooLarge: LocaleFn<[string]>;
    notAnImage: LocaleFn<[string]>;
  };
  /** 终端面板（GhosttyTerminal）：标签栏、工具按钮 tooltip、会话状态与右键菜单。 */
  terminal: {
    // ---- 标签栏按钮 tooltip ----
    closeTab: string;
    newTab: string;
    gripDrag: string;
    moveToBottom: string;
    dockToRight: string;
    restoreHeight: string;
    maximizeHeight: string;
    minimizeClosePanel: string;
    // ---- 会话状态 ----
    starting: string;
    shellExited: string;
    abnormalExit: LocaleFn<[string]>;
    startFailed: LocaleFn<[string]>;
    engineLoading: string;
    engineUnavailable: LocaleFn<[string]>;
    timeout: LocaleFn<[string, number]>;
    sessionInfo: LocaleFn<[string, string]>;
    // ---- 会话命名 ----
    tabName: LocaleFn<[number]>;
    /** 单项目标签到达上限时的提示（参数为上限值） */
    tabLimitReached: LocaleFn<[number]>;
    /** 全窗口 PTY 预算耗尽，新增/分屏被拒绝 */
    consoleBudgetExceeded: string;
    // ---- 标签右键菜单 ----
    copyWorkingDirectory: string;
    splitRight: string;
    splitDown: string;
    closePane: string;
    close: string;
    closeOthers: string;
    closeAll: string;
    rename: string;
    // ---- 搜索 ----
    searchPlaceholder: string;
    searchPrev: string;
    searchNext: string;
    searchClose: string;
    /** 屏幕阅读器读取终端视口时的区域名 */
    a11yLabel: string;
    /** 隐藏输入框的无障碍名称 */
    a11yInputLabel: string;
  };
  /** 设置面板自身（SettingsDialog）的文案。section 名与 settings 区分：后者只有语言项。 */
  settingsPanel: {
    closeEsc: string;
    appearanceTab: string;
    terminalTab: string;
    editorTab: string;
    filesTab: string;
    aboutTab: string;
    updatesTab: string;
    updatesSection: string;
    localDiagnostics: string;
    localDiagnosticsDesc: string;
    autoCheckUpdates: string;
    autoCheckUpdatesDesc: string;
    checkNow: string;
    lastChecked: LocaleFn<[string]>;
    neverChecked: string;
    ignoredFilesTitle: string;
    ignoredFilesDesc: string;
    ignoredPatternPlaceholder: string;
    addPatternBtn: string;
    restoreDefaultPatterns: string;
    patternAlreadyExists: string;
    uiVariantLabel: string;
    editorThemeLabel: string;
    windowDecoration: string;
    windowDecorationHint: string;
    captionMacos: string;
    captionWindows: string;
    uiFontSizeLabel: string;
    uiFontSizeHint: string;
    uiFontSizeDefault: string;
    terminalEngine: string;
    terminalFontSizeLabel: string;
    terminalFontSizeHint: string;
    editorFontSizeLabel: string;
    editorFontSizeHint: string;
    engineNotDetected: string;
    themeCount: LocaleFn<[number]>;
    // ---- 编辑器配色与界面变体的展示名（模块级常量，需渲染时取当前语言）----
    themeFollowUi: string;
    themeFollowUiDesc: string;
    themeIdeDark: string;
    themeIdeDarkDesc: string;
    themeIdeLight: string;
    themeIdeLightDesc: string;
    variantNativeDark: string;
    variantNativeDarkTag: string;
    variantNativeDarkDesc: string;
    variantNativeLight: string;
    variantNativeLightTag: string;
    variantNativeLightDesc: string;
    settingsTitle: string;
    terminalEngineHint: string;
    aboutTerminalEngine: string;
    aboutLocalGhostty: string;
    aboutRenderPipeline: string;
    aboutStack: LocaleFn<[string]>;
    /** 从 VSCode 设置导入（.vscode/settings.json） */
    vscodeImport: string;
    vscodeImportHint: string;
    vscodeImportNoWorkspace: string;
    vscodeImportNone: string;
    vscodeImportOk: LocaleFn<[number, string]>;
    vscodeImportFailed: LocaleFn<[string]>;
    projectOverrides: string;
    projectOverridesHint: LocaleFn<[number]>;
    clearProjectOverrides: string;
  };
  /** 设置面板内与本功能直接相关的部分。 */
  settings: {
    language: string;
    languageHint: string;
    english: string;
    chineseSimplified: string;
  };
  /** 通用动词/名词，散落在多处按钮与 tooltip。 */
  common: {
    close: string;
    minimize: string;
    maximize: string;
    restore: string;
    loading: string;
    yes: string;
    no: string;
    none: string;
    default: string;
    retry: string;
    copy: string;
    copied: string;
    pathNotFound: string;
  };
  /** Git 底部面板（GitBottomPanel）：页签、分支树、日志过滤栏、提交详情、右键菜单。 */
  gitPanel: {
    /** 页签与面板标题 */
    tabTitle: string;
    tabLog: string;
    tabConsole: string;
    /** 窗口动作 */
    maximizeRestore: string;
    /** 非 git 仓库横幅 */
    notRepoText: string;
    initRepo: string;
    /** 分支树 */
    headCurrentBranch: string;
    local: string;
    remote: string;
    branchCheckoutHint: string;
    branchCheckoutTrackHint: string;
    branchKindLocal: string;
    branchKindRemote: string;
    tags: string;
    noMatchingBranches: string;
    branchSearchPlaceholder: string;
    branchToolbar: {
      collapse: string;
      hideBranches: string;
      showBranches: string;
      fetch: string;
      newBranch: string;
      checkout: string;
      delete: string;
      update: string;
      search: string;
      showDiff: string;
      favorite: string;
      compare: string;
      viewOptions: string;
      groupByDirectory: string;
      expandAll: string;
      collapseAll: string;
    };
    resizeBranches: string;
    resizeDetails: string;
    resizeDetailsSplit: string;
    /** 合并冲突横幅 */
    conflictBanner: LocaleFn<[number]>;
    resolveConflicts: string;
    abortMerge: string;
    abortCherryPick: string;
    continueCherryPick: string;
    cherryPickPending: string;
    abortMergeTitle: string;
    abortMergeConfirm: string;
    abortCherryPickTitle: string;
    abortCherryPickConfirm: string;
    mergeAborted: string;
    mergeContinued: string;
    cherryPickAborted: string;
    cherryPickContinued: string;
    abortRebaseTitle: string;
    abortRebaseConfirm: string;
    rebaseAborted: string;
    rebaseContinued: string;
    skipRebaseTitle: string;
    skipRebaseConfirm: string;
    skipRebaseOk: string;
    rebaseSkipped: string;
    /** 顶部操作 pill（对齐 PyCharm 的 Cherry-picking in <branch> ≫ ✕） */
    opMerging: LocaleFn<[string]>;
    opMergingBare: string;
    opRebasing: LocaleFn<[string]>;
    opRebasingBare: string;
    opCherryPicking: LocaleFn<[string]>;
    opCherryPickingBare: string;
    opContinueTitle: string;
    /** 日志过滤栏 */
    filterBranch: string;
    filterUser: string;
    filterDate: string;
    filterPaths: string;
    filterTitle: LocaleFn<[string]>;
    allBranches: string;
    allAuthors: string;
    dateAll: string;
    dateToday: string;
    dateLast7Days: string;
    dateLast30Days: string;
    searchPlaceholder: string;
    pathPlaceholder: string;
    regexTooltip: string;
    matchCaseTooltip: string;
    apply: string;
    clear: string;
    /** 过滤栏右侧的 Git 动作按钮 */
    pushTooltip: string;
    updateProjectTooltip: string;
    fetchTooltip: string;
    stashTooltip: string;
    unstashTooltip: string;
    shelfTooltip: string;
    refreshTooltip: string;
    refreshingTooltip: string;
    moreActions: string;
    sortTooltip: string;
    sortBy: string;
    byCommitDate: string;
    topologically: string;
    options: string;
    firstParent: string;
    noMerges: string;
    branchActions: string;
    collapseLinear: string;
    expandLinear: string;
    showGraph: string;
    showBranches: string;
    previewDiff: string;
    searchByHash: string;
    /** 提交行 */
    rowHint: string;
    refBranch: LocaleFn<[string]>;
    refRemoteBranch: LocaleFn<[string]>;
    refTag: LocaleFn<[string]>;
    refStash: LocaleFn<[string]>;
    refHead: string;
    loadMore: string;
    /** 提交详情（文件清单 / 提交信息） */
    filesLoading: string;
    filesChanged: LocaleFn<[number]>;
    noFileChanges: string;
    noCommitSelected: string;
    fileDiffHint: LocaleFn<[string]>;
    revertTooltip: string;
    showCommitDiffTooltip: string;
    /** Console 页签 */
    consoleEmpty: string;
    /** 提交详情文件右键菜单 */
    ctxShowDiff: string;
    ctxCompareWithLocal: string;
    ctxGetFromRevision: string;
    ctxCopyPatch: string;
    ctxCreatePatch: string;
  };
  /** Commit 工具窗口（CommitDrawer）：页签、工具栏、右键菜单、删除/Unshelve 弹窗。 */
  commitDrawer: {
    // ---- 页签与面板动作 ----
    tabCommit: string;
    tabShelf: string;
    hidePanel: string;
    // ---- 工具栏 ----
    refreshTooltip: string;
    refreshing: string;
    rollbackSelectedTooltip: string;
    shelveSelectedTooltip: string;
    folderViewTooltip: string;
    expandAll: string;
    collapseAll: string;
    viewOptions: string;
    groupBy: string;
    groupByDirectory: string;
    show: string;
    showIgnoredFiles: string;
    ignoredFiles: string;
    layout: string;
    showDetails: string;
    showDiffNewTab: string;
    compareWithLocal: string;
    compareBeforeLocal: string;
    openRepositoryVersion: string;
    revertSelectedChanges: string;
    cherryPickSelectedChanges: string;
    extractSelectedChanges: string;
    dropSelectedChanges: string;
    getFromRevision: string;
    historyUpToHere: string;
    showChangesToParents: string;
    // ---- 空态 / 横幅 ----
    notRepoNotice: string;
    readingChanges: string;
    worktreeClean: string;
    conflictFiles: LocaleFn<[number]>;
    resolveConflicts: string;
    mergeConflictsTitle: string;
    resolveLink: string;
    unversionedFiles: string;
    // ---- 文件行 ----
    fileRowHint: LocaleFn<[string]>;
    fileRowHintPlain: LocaleFn<[string]>;
    rollbackFileTooltip: string;
    fileCount: LocaleFn<[number]>;
    // ---- Shelf 贮藏架 ----
    shelfGroupHint: LocaleFn<[string]>;
    shelfTreeHint: LocaleFn<[string]>;
    shelfCount: LocaleFn<[number, string]>;
    shelfEmptyHint: string;
    unshelveSelectedTooltip: string;
    shelfDeleted: LocaleFn<[string]>;
    shelfDeleteFailed: LocaleFn<[string]>;
    // ---- 右键菜单 ----
    unshelveMenu: string;
    deleteShelfMenu: string;
    newChangelistPrompt: string;
    newChangelist: string;
    deleteChangelist: string;
    addToVcs: string;
    addFilesToVcs: LocaleFn<[number]>;
    editSource: string;
    jumpToSource: string;
    deleteAction: string;
    deleteFilesAction: LocaleFn<[number]>;
    commitFileOnly: string;
    commitFilesAction: string;
    showDiffAction: string;
    moveToChangelist: string;
    copyPatch: string;
    createPatch: string;
    shelveAction: string;
    rollbackAction: string;
    refreshChanges: string;
    // ---- 删除 changelist 弹窗 ----
    deleteChangelistTitle: string;
    deleteChangelistMsg: LocaleFn<[string, number]>;
    deleteChangelistMove: string;
    deleteChangelistRollback: string;
    // ---- Unshelve 弹窗 ----
    unshelveTitle: string;
    unshelveMsg: string;
    unshelveNewTag: string;
    newChangelistLabel: string;
    newChangelistPlaceholder: string;
    unshelve: string;
    // ---- 提交区 ----
    amend: string;
    historyTooltip: string;
    noHistoryTooltip: string;
    historyHeader: string;
    clearHistory: string;
    messagePlaceholder: string;
    committing: string;
    commit: string;
    commitAndPush: string;
  };
  /** 标题栏（TitleBar）：项目下拉、分支下拉、窗口动作按钮。 */
  titleBar: {
    projectPillTooltip: string;
    searchProjectsPlaceholder: string;
    searchBranchesPlaceholder: string;
    openNewProject: string;
    openProjectsSection: string;
    currentProjectTitle: string;
    switchProjectTitle: string;
    otherWindowTitle: string;
    openedInOtherWindow: string;
    recentProjectsSection: string;
    noRecentProjects: string;
    openThisProject: string;
    removeRecentTitle: string;
    branchPillTooltip: string;
    filterBranchesPlaceholder: string;
    newBranchNamePlaceholder: string;
    createAndSwitch: string;
    newBranchAction: string;
    localBranchesSection: string;
    noMatchingLocalBranches: string;
    remoteBranchesSection: string;
    dragResizeMenuHeight: string;
    searchEverywhere: string;
    deleteTagTitle: string;
    noTags: string;
    recentSection: string;
    localSection: string;
    remoteSection: string;
    tagsSection: string;
    checkoutTagOrRevision: string;
    continueRebaseAction: string;
    skipRebaseAction: string;
    abortRebaseAction: string;
  };
  /** 项目树右键菜单（ProjectTreeContextMenu）。 */
  treeMenu: {
    newSubmenu: string;
    newFile: string;
    newDirectory: string;
    copyPathReference: string;
    absolutePath: string;
    relativePath: string;
    pathFromRepoRoot: string;
    notInRepoTitle: string;
    noRepoRootTitle: string;
    isRepoRootTitle: string;
    gitSubmenu: string;
    addToVcs: string;
    addToVcsDir: string;
    rollbackDir: string;
    showDiff: string;
    showHistory: string;
    annotateBlame: string;
    rollbackFile: string;
    openInSubmenu: string;
    finder: string;
    terminal: string;
    deleteItems: LocaleFn<[number]>;
    rename: string;
    findInFiles: string;
    expandAll: string;
    collapseAll: string;
    reloadFromDisk: string;
    compareWith: string;
  };
  /** 欢迎页（WelcomeView）：品牌语、快捷入口、最近项目、快捷键提示。 */
  welcome: {
    subtitle: string;
    quickStart: string;
    openWorkspace: string;
    openWorkspaceSub: string;
    newWindow: string;
    newWindowSub: string;
    globalSearch: string;
    globalSearchSub: string;
    openTerminal: string;
    openTerminalSub: string;
    settings: string;
    settingsSub: string;
    recentProjects: string;
    projectCount: LocaleFn<[number]>;
    noRecentProjects: string;
    openDirNow: string;
    removeFromRecords: string;
    searchHint: string;
    settingsHint: string;
    closeDialogHint: string;
  };
  /** 编辑器标签栏（EditorTabBar）：标签 tooltip 与标签右键菜单。 */
  editorTab: {
    dirtyTitle: string;
    closeTab: string;
    diffTabTitle: string;
    closeDiffTab: string;
    diffTabName: LocaleFn<[string]>;
    diffCommitsTabName: LocaleFn<[number]>;
    workingTreeDiff: string;
    closeCurrent: string;
    closeOthers: string;
    closeLeft: string;
    closeRight: string;
    closeAll: string;
    moreTabs: string;
  };
  /** 编辑器右键菜单（EditorContextMenu）。 */
  editorMenu: {
    goToDefinition: string;
    findReferences: string;
    renameSymbol: string;
    formatDocument: string;
    showHistory: string;
    annotateBlame: string;
  };
  /** 左侧工具条（LeftStrip）：工具窗口按钮 tooltip。 */
  leftStrip: {
    projectTooltip: string;
    commitTooltip: string;
    commitTooltipWithChanges: LocaleFn<[number]>;
    gitLogTooltip: string;
    terminalTooltip: string;
    terminalTooltipRunning: LocaleFn<[string]>;
    settingsTooltip: string;
  };
  /** 分支右键 / 分支下拉操作子菜单（BranchContextMenu）。 */
  /** 提交右键菜单（CommitContextMenu）。 */
  commitMenu: {
    copyRevision: string;
    createPatch: string;
    cherryPick: LocaleFn<[string]>;
    checkoutRevision: LocaleFn<[string]>;
    resetHere: string;
    revert: LocaleFn<[string]>;
    undo: LocaleFn<[string]>;
    editMessage: string;
    drop: LocaleFn<[string]>;
    newBranchFrom: LocaleFn<[string]>;
    newTagFrom: LocaleFn<[string]>;
    copyPatch: string;
    interactiveRebase: string;
    goToChild: string;
    goToParent: string;
    showRepoAtRevision: string;
    compareVersions: string;
    revertCommits: string;
    dropCommits: string;
    squashCommits: string;
    pushUpToHere: string;
  };
  branchMenu: {
    checkoutRemoteLocal: LocaleFn<[string]>;
    checkout: LocaleFn<[string]>;
    newBranchFrom: LocaleFn<[string]>;
    showDiff: string;
    push: string;
    mergeInto: LocaleFn<[string, string]>;
    rebaseOnto: LocaleFn<[string, string]>;
    pullMerge: LocaleFn<[string]>;
    pullRebase: LocaleFn<[string]>;
    rename: string;
    delete: LocaleFn<[string]>;
    deleteRemote: LocaleFn<[string]>;
    compare: string;
    updatePlain: string;
    updateCurrent: string;
    updateBranch: LocaleFn<[string]>;
    favorite: string;
    unfavorite: string;
    checkoutPlain: string;
    checkoutAndRebase: LocaleFn<[string, string]>;
    compareBranches: string;
    showFilesDiff: string;
    updateSelected: string;
    deleteBranches: string;
    pushTag: string;
    deletePlain: string;
    deleteTag: LocaleFn<[string]>;
  };
  /** Shelf 搁置列表右键菜单（对齐 PyCharm Shelf 列表与文件上下文菜单） */
  shelfMenu: {
    unshelve: string;
    restore: string;
    showDiff: string;
    showDiffInNewTab: string;
    compareWithLocal: string;
    createPatch: string;
    copyPatch: string;
    importPatches: string;
    rename: string;
    delete: string;
    renamePrompt: string;
    deleteShelfConfirm: LocaleFn<[string]>;
    deleteFileConfirm: LocaleFn<[string, string]>;
    patchCopied: string;
    imported: LocaleFn<[string]>;
    restored: LocaleFn<[string]>;
    recentlyDeleted: string;
    cleanRecentlyDeleted: string;
  };
  /** Markdown 查看器（MarkdownView）：视图模式切换与分栏拖拽。 */
  markdownView: {
    title: string;
    modeSwitcherLabel: string;
    source: string;
    sourceTitle: string;
    preview: string;
    previewTitle: string;
    split: string;
    splitTitle: string;
    resizerLabel: string;
    resizerTitle: string;
    loadRemoteImage: string;
    remoteImageHint: LocaleFn<[string]>;
  };
  /** 首屏渲染异常兜底页（main.tsx 的 ErrorBoundary）。 */
  errorPage: {
    title: string;
    unknownError: string;
    reload: string;
    recover: string;
    /** variant 样式文件缺失（构建产物不全时才会出现） */
    variantMissing: LocaleFn<[number]>;
    /** 控制台错误日志：挂载 variant 样式失败 */
    applyVariantFailed: string;
  };
  /** 浏览器预览（无原生后端）时 IPC 示意数据的文案：返回值与抛出的错误会经
   *  toast / 终端错误区 / 编辑器正文展示给用户，必须跟随界面语言。 */
  preview: {
    /** 示意 git 日志的提交信息（假数据，仅浏览器预览） */
    sampleCommit: LocaleFn<[number]>;
    /** read_file_content 的示意正文，原样进编辑器 */
    sampleFile: string;
    notExecutedInit: string;
    notExecutedStash: string;
    notExecutedPush: string;
    notExecutedPull: string;
    notExecutedFetch: string;
    notExecutedCommit: string;
    /** 抛出的错误：文案会经 showStatus / 终端错误区显示 */
    ghosttyUnavailable: string;
    terminalUnavailable: string;
    imageUnavailable: string;
  };
  updater: {
    checkingTitle: string;
    checkingDesc: string;
    upToDateTitle: string;
    upToDateDesc: LocaleFn<[string]>;
    upToDateCheckedAt: LocaleFn<[string]>;
    updateAvailableTitle: string;
    newVersionAvailable: LocaleFn<[string, string]>;
    releaseDate: LocaleFn<[string]>;
    releaseNotes: string;
    ignoreThisVersion: string;
    remindMeLater: string;
    updateAndRestart: string;
    downloadingTitle: string;
    downloadingProgress: LocaleFn<[string, string, number]>;
    updateReadyTitle: string;
    updateReadyDesc: string;
    restartNow: string;
    restartLater: string;
    checkFailedTitle: string;
    checkFailedDesc: LocaleFn<[string]>;
    toastTitle: string;
    toastDesc: LocaleFn<[string]>;
    toastActionUpdate: string;
    toastActionDismiss: string;
    topbarBadge: LocaleFn<[string]>;
    topbarBadgeTitle: LocaleFn<[string]>;
    saveBeforeRestartTitle: string;
    saveBeforeRestartDesc: string;
  };
}
