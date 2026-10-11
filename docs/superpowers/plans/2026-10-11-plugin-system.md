# Phase 3A 插件系统全链路（双作用域 + 主区插件 Tabs）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 插件从安装（local/git/市场）到 consent/启用/worker/面板/主区 tab 全链路可用；双作用域（全局 + 项目 lockfile 一键安装）；插件面板可经 `workspace.openTab` 打开主区 tab。

**Architecture:** 新 crate `ade-plugins`（纯 Rust：manifest/hash/安装/consent/worker/host methods/市场/watcher/service）→ ade-bridge ~22 命令 + `plugins:changed`/`plugins:open-tab` 事件 → TS `real/plugins.ts` 21 方法 + 契约扩展（scope/tabs/lock）→ 渲染层（作用域/安装选择/lockfile 提示/市场/开发中/主区 tabs）。Worker = 系统 `node` 跑资源 `host-entry.mjs`（NDJSON stdio 帧）。参照实现 `/Users/itsuka/CodeSpace/orca`（只读），`orca:` 前缀均为其内路径。

**Tech Stack:** Rust（ade-plugins：sha2/hex/walkdir/notify/keyring/serde/serde_json/thiserror/uuid/libc + ade-core）、Node（worker 宿主，系统 `node>=18`）、TypeScript（Vitest、Tauri IPC/specta）。

**Spec:** `docs/superpowers/specs/2026-10-11-plugin-system-design.md`（执行者须同时阅读）

## Global Constraints

- Rust 依赖：`ade-plugins` 新增 `serde/serde_json/thiserror/uuid/libc/sha2/hex/walkdir/notify/keyring`（notify 用工作区 ade-fs 既有版本）；**不新增任何 npm 依赖**（host-entry.mjs 纯 Node 内置模块）。
- 新 crate 放 `src-tauri/crates/ade-plugins/`（自动入 workspace）；**不依赖 tauri/specta/ade-store**（settings/secret/服务经 trait 由 ade-bridge 注入）。
- bindings 唯一生成方式：`cargo run -p ade-bridge --bin export-bindings`（workdir `src-tauri`）；`bindings_are_fresh` 校验；新命令名进 `specta_export.rs` 两处清单。
- 提交信息中文 conventional + `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- cargo 命令在 `src-tauri` 下执行；pnpm 在仓库根。
- 仅 macOS/POSIX；可选项省略不物化 null。
- oracle 逐字对齐面：错误码集合、consent 指纹算法、host API 参数/结果 schema 与限额、worker 帧字段与超时值、shell CSP/预算——**执行时从 `orca:` 参照文件复制，不凭记忆写**。
- **测试不得依赖网络**：git/市场测试用本地临时仓库；worker 测试用 fake node 脚本；secrets 用 fake trait（真 Keychain 仅手工验收）。
- 渲染层既有插件测试全绿是硬门禁（除按计划显式迁移 scope fixtures 类型）。

---

### Task 1: crate 骨架 + manifest + content_hash

**Files:**
- Create: `src-tauri/crates/ade-plugins/Cargo.toml`
- Create: `src-tauri/crates/ade-plugins/src/lib.rs`
- Create: `src-tauri/crates/ade-plugins/src/types.rs`（scope/key/capability/错误码）
- Create: `src-tauri/crates/ade-plugins/src/manifest.rs`
- Create: `src-tauri/crates/ade-plugins/src/content_hash.rs`
- Test: 同 crate `#[cfg(test)]`

**Interfaces:**
- Consumes: 无（首任务）。
- Produces（名称固定）：
  - `types::{PluginScope, Capability, PluginError}`；`PluginScope` serde `global|project`；`Capability` 8 值（7 + `workspace:open-tab`，`as_str`/canonical 序）；`PluginError{code:&'static str, message:String}` + `code()` 供 UI 映射。
  - `manifest::Manifest{ id, publisher, name, version, description, author, repository?, icon?, engines_orca, plugin_api, main?, contributes, capabilities }`；`Contributes{ panels, tabs, commands, events, language_packs, keybindings, vm_recipes, agents }`（`PanelContrib{id,title,icon?,entry}`、`TabContrib` 同形、上限照 oracle + tabs ≤64）；`manifest::parse_manifest(bytes: &[u8]) -> Result<Manifest, PluginError>`（`ade-plugin.json` 主名由调用方定；≤1MiB 由读取层校验）；`Manifest::qualified_key() -> String`；`manifest::engine_satisfies(range, version) -> bool`（仅 `>=x.y.z`）。
  - `content_hash::hash_plugin_tree(root: &Path) -> Result<String, PluginError>`（`orca-plugin-tree-v1\0` + 8B BE 帧 + 排序路径；symlink 拒；跳根 `.git`；≤2000 文件/50MiB）。

- [ ] **Step 1: 写失败测试**
  - manifest：合法样例（含 tabs）解析；缺字段/超长/非法 id/保留名/非 semver/engines 非 `>=`/capability 未知/contribution 超上限/entry 跨 `..` 越界 → 对应 `PluginError.code`（文案抄 `orca:src/shared/plugins/plugin-manifest*.ts` 校验语义）。
  - canonical capability 序列化去重；engine gate 边界（`>=1.0.0` vs pre-release）。
  - content_hash：固定小树（文件/子目录/空文件）对齐 oracle 算法（用 oracle 测试向量或手算）；symbolic link 拒绝；`.git` 跳过；超 2000 文件报错。
- [ ] **Step 2: RED → 实现 → GREEN**

Run: `cargo test -p ade-plugins`

- [ ] **Step 3: 提交** `feat(plugins): ade-plugins 骨架、manifest 与内容哈希`

---

### Task 2: discovery + artifact + path 安全

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/paths.rs`
- Create: `src-tauri/crates/ade-plugins/src/artifact.rs`
- Create: `src-tauri/crates/ade-plugins/src/discovery.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `paths::{ScopeRoots{install_dir,data_dir,lockfile_path}, global_roots(app_data_dir), project_roots(repo_path), validate_containment(base, candidate) -> Result<PathBuf, PluginError>}`（项目根 = `<repo>/.ade`；全局根 = `<app_data_dir>`）。
  - `artifact::validate_declared_artifacts(root, &Manifest) -> Result<ArtifactSet, PluginError>`：realpath 包含 + 单类上限（worker 50MiB/panel 10MiB/**tab 10MiB**/icon 2MiB/languagePack 5MiB/vmRecipe 256KiB/agent 1MiB），返回各产物绝对路径。
  - `discovery::{DiscoveredPlugin{key, scope, repo_id?, root, manifest, source_kind: installed|dev|bundled, install_hash?}, discover(global, projects: &[(repo_id, path)], dev_paths: &[PathBuf]) -> Vec<DiscoveredPlugin>}`：dev 遮罩 installed（同 scope）；项目遮罩全局同 key（投影层标志）；同 scope 同 key 重复 → invalid 项（含 error）；列表投影不泄 dev 绝对路径（`DiscoveryProjection`）。
- [ ] **Step 1: 写失败测试**：tempdir 三根（全局/两项目/dev 两路径）；遮罩矩阵；重复 key；artifact 越界（symlink 逃逸/超限）；containment。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): 发现、产物校验与路径安全`

---

### Task 3: 安装管线（staging/发布/lockfile/回滚/卸载）

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/install/{mod.rs,atomic_write.rs,pointer.rs,provenance.rs,lockfile.rs,staging.rs,publication.rs}`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `install::atomic_write::write_atomic(path, bytes, mode)`（tmp+rename；EPERM/EACCES/EBUSY 重试）。
  - `install::pointer::{read_pointer(key_dir)->Option<String>, write_pointer}`（≤128B）。
  - `install::provenance::{write_provenance(install_root, hash, &Provenance), read_provenance}`（0600，`.install-provenance/<hash>.json`）。
  - `install::lockfile::{LockEntry{plugin_key,version,source:LockSource,resolved_commit?,content_hash,consent_fingerprint,installed_at}, LockSource::{LocalPath{path},Git{url,ref_},Marketplace{..},Bundled{..}}, read_lockfile(path)->Lockfile(损坏→空), write_lockfile_atomic, upsert_entry, remove_entry}`（序列化含 legacy `capabilityHash` 回读；git URL 白名单 `is_plugin_git_url`）。
  - `install::staging::{stage_plugin_tree(source_root, dest_parent, manifest) -> (hash, staged_dir)}`：过滤 `.git`、cp、rehash、reparse、revalidate；`<hash>` 已存在→校验既有字节。
  - `install::publication::{publish_install(roots, key, hash, provenance, lock_entry, lockfile_path) -> Result<()>}`：provenance→指针→lockfile；lockfile 失败回滚旧指针；保留 current+1 清理其余。
  - `install::{install_from_local(root, roots, scope_ctx) -> InstallResult, rollback(key, roots), uninstall(key, roots) -> {removed_install, removed_data}}`；`InstallResult{plugin_key, version, content_hash, consent_fingerprint, install_hash, scope}`。
- [ ] **Step 1: 写失败测试**（锚 `orca:src/main/plugins/plugin-install*.test.ts`）：本地安装全流程（staging→发布→指针→lock 条目）；重复安装同 hash 幂等；lockfile 损坏回空；lockfile 写失败指针回滚（注入失败）；回滚（当前+1，其余 prune）；卸载清理+lock 移除；项目根 `.ade/.gitignore` deny 写入（`plugins/`、`plugin-data/`，已存在不覆盖）。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): 安装管线（staging/发布/lockfile/回滚/卸载）`

---

### Task 4: consent + 启用 + 完整性

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/consent.rs`
- Create: `src-tauri/crates/ade-plugins/src/enablement.rs`
- Create: `src-tauri/crates/ade-plugins/src/integrity.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `consent::{ConsentState trait(注入 main-owned settings), scoped_key(key, scope, repo_id)->String`（`global:<key>`/`project:<repo>:<key>`), fingerprint(manifest, content_key:&str)->String, ActivationState::{Disabled,Pending,Approved}, activation_state(state, scoped_key, fingerprint)}`；指纹算法逐字移植 `orca:src/shared/plugins/plugin-consent-fingerprint.ts`（capabilities 规范化 + `\0trusted-node-worker` + instructional（keybindings/vmRecipes/agents）+ 内容键）。
  - `enablement::{approve(state, scoped_key, reviewed_fingerprint, current_fingerprint)->Result<, PluginError>, set_enabled(state, scoped_key, enabled)}`：approve 指纹不一致拒绝（`consent_stale`）；enable 不绕过。
  - `integrity::verify_installed(install_root, &Manifest) -> Result<(), PluginError>`（重算内容哈希对比 manifest/指针；失败 `integrity_mismatch`）。
- [ ] **Step 1: 写失败测试**：指纹对照 oracle 测试向量（`orca:src/shared/plugins/plugin-consent-fingerprint.test.ts` 的用例移植）；激活态三态与 needsReconsent；approve 指纹竞态拒绝；integrity 篡改检测。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): consent 指纹、启用与完整性校验`

---

### Task 5: storage + settings + audit + secrets（Keychain）

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/storage.rs`
- Create: `src-tauri/crates/ade-plugins/src/plugin_settings.rs`
- Create: `src-tauri/crates/ade-plugins/src/audit.rs`
- Create: `src-tauri/crates/ade-plugins/src/secrets.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `storage::StorageStore::new(data_dir)`：per-plugin `storage.json`（0600）；`get/set/delete/keys`；保留键 `__proto__/prototype/constructor` 拒绝；值 ≤256KiB JSON、总 ≤5MiB、≤1024 keys（错误码照 host-api）。
  - `plugin_settings::PluginSettingsStore`：per-plugin `settings.json`（0600）；`get_all/set`。
  - `audit::{AuditLog::new(data_dir), record(AuditRecord{ts,actor,method,summary,outcome}), read_tail(key, limit)->Vec<LogLine>}`（JSONL，10MiB 轮转）。
  - `secrets::{SecretStore trait{get,set,delete}, KeychainSecrets::new(service)`（keyring：service `dev.itsuka.orcinus.plugin-secrets`，account `<pluginKey>:<key>`）, fail-closed 错误 `unavailable`}`；set 值 ≤64KiB。
- [ ] **Step 1: 写失败测试**（fake keychain 内存实现 + tempdir）：storage 限额/保留键/持久化；audit 轮转与读取；secrets 经 trait 的读写删 + keychain 错误映射。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): 存储、插件设置、审计与 Keychain secrets`

---

### Task 6: worker 帧协议 + host-entry.mjs

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/worker/protocol.rs`
- Create: `src-tauri/crates/ade-plugins/src/worker/mod.rs`
- Create: `src-tauri/resources/plugins/host-entry.mjs`
- Modify: `src-tauri/tauri.conf.json`（`bundle.resources` 增 `"resources/plugins/": "plugins/"`）
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `worker::protocol::{ParentFrame::{Init{plugin_id,plugin_root,main_entry,granted_capabilities},InvokeCommand{call_id,command_id,args},DeliverEvent{event_id,event,payload},HostResult{call_id,ok,value,error_code,error},Shutdown}, ChildFrame::{Ready{commands},CommandResult{call_id,ok,value,error},EventAck{event_id},HostCall{call_id,method,params},Log{level,message},Fatal{error}}, read_frame/write_frame(NDJSON ≤1MiB, error ≤8192 截断)`（字段名照 `orca:src/shared/plugins/plugin-host-protocol.ts`，serde camelCase）。
  - `host-entry.mjs`：stdin 逐行解析 ParentFrame；file-URL import `<pluginRoot>/<mainEntry>`；默认导出/SDK `activate(orca)`；SDK `{commands.register, events.on, host.call, grantedCapabilities, log}`；`ready` 于 activate 后发出；`deactivate` on shutdown 后 exit(0)；未捕获→fatal+exit(1)；`host.call` 经 HostCall/CallResult 关联（超时由父侧控制）。
- [ ] **Step 1: 写失败测试**：Rust 帧 round-trip 与截断；hello-plugin fixture（临时 JS 插件：activate 注册 1 命令、echo host.call、deactivate 打点）→ 用 `node`（`which node` 可得则跑，缺失则 `#[ignore]` 并记录）断言 ready/commandResult/hostCall/log/shutdown 流。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`；并验证 `cargo check -p orcinus-app` 仍通过（resources 目录含已提交的 host-entry.mjs）。
- [ ] **Step 3: 提交** `feat(plugins): worker 帧协议与 host-entry.mjs 宿主`

---

### Task 7: worker spawn + supervisor + manager

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/worker/spawn.rs`
- Create: `src-tauri/crates/ade-plugins/src/worker/supervisor.rs`
- Create: `src-tauri/crates/ade-plugins/src/worker/manager.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `worker::spawn::{resolve_node(env_override, hydrated_path)->Result<PathBuf, PluginError>, node_supports(version)->bool(>=18), WorkerSpec{plugin_key,plugin_root,main_entry,granted_capabilities,manifest_revision}, spawn_worker(spec, host_entry_path, node) -> WorkerHandle{pid, stdin_write, stdout_rx, kill(grace)}`（env 白名单 PATH/HOME/LANG/TZ/TMP*；`Command` 进程组；SIGTERM→2s→SIGKILL，仿 `ade-automations/src/precheck.rs`）。
  - `worker::supervisor::{WorkerSupervisor, state: WorkerState::{Inactive,Running{revision},Restarting,Errored{reason}}, 限额: ready 10s/invoke 30s/event 5min/idle 5min/grace 2s/重启≤3 backoff[500,2000,5000]ms}`。
  - `worker::manager::{WorkerManager, ensure_active(key), invoke_command(key,command_id,args)->Result<Value>, deliver_event(...), stop(key), stop_all(), active_count()<=5, revision 变更重启, undeclared command→deactivate+errored}`。
- [ ] **Step 1: 写失败测试**（fake node = 测试内联 `.mjs` 脚本，经 env 注入 spawn 参数）：lazy 激活；invoke 超时；event ack 超时；崩溃重启计数与 backoff；超限 errored；max 5 slot；revision 重启；undeclared command 处理；shutdown 清理无泄漏进程。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): worker spawn、监督与池化`

---

### Task 8: host gate + methods（14）

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/host/mod.rs`
- Create: `src-tauri/crates/ade-plugins/src/host/gate.rs`
- Create: `src-tauri/crates/ade-plugins/src/host/api.rs`（14 方法 schema/限额）
- Create: `src-tauri/crates/ade-plugins/src/host/methods.rs`
- Create: `src-tauri/crates/ade-plugins/src/host/services.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `host::gate::decide(granted: Option<&[Capability]>, via_panel: bool, method: &str) -> GateDecision::{Granted, Denied{code,error}}`（`unknown_method/panel_forbidden/consent_required/capability_denied`）。
  - `host::api::{MethodSpec{name,capability,mutation,panel_callable,validate_params,validate_result}, specs()->14 项（13 照 `orca:src/shared/plugins/plugin-host-api.ts` 表 + workspace.openTab）}`。
  - `host::methods::execute(method, params, ctx: HostCallContext{plugin_key, granted, via_panel, services:&dyn PluginHostServices}) -> Outcome::{Ok(Value), Err{code,error}}`：参数/结果双端校验；mutation 必过 audit（attempt→ok/error）。
  - `host::services::PluginHostServices: Send+Sync trait`：`active_worktree_context()->Option<WorktreeContext{worktree_id,branch,display_name}>, list_worktree_terminals(wt)->Vec<TerminalRef{id}> , send_terminal_text(id,text,enter)->{accepted}, dispatch_notification(plugin_key,title,body)->{delivered}, storage/settings/secrets 访问器, subscribe_events(plugin_key,events)->Vec<&str>, open_tab(plugin_key, tab_id,title?,params, scope, repo_id?, wt)->Result<OpenTabResult{instance_id,worktree_id},HostError>}`。
  - `workspace.readContext`/`terminal.sendText` 的 worktree 解析：`ctx.worktree_id` 优先（tab 会话），否则 `services.active_worktree_context()`；终端 id 白名单复核。
  - `workspace.openTab`：校验 tabId ∈ 该插件该 scope 的 contributes.tabs；params ≤64KiB（JSON 深度/大小校验）；title ≤128；去重（`(plugin_key,tab_id,params)` 深比较）→ 由 services 回调实现；限额（8/插件/worktree、4/10s rate）在 service 层。
- [ ] **Step 1: 写失败测试**（fake services）：gate 矩阵（8 capability × panel/worker × 未知方法）；14 方法参数/结果校验与错误码；readContext/terminal 的 worktree 与白名单；openTab 声明校验/参数限额；audit 顺序（attempt 先于 handler、失败路径 error）。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): host gate 与 14 个 host methods`

---

### Task 9: 会话（面板/主区 tab/全局视图）

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/sessions.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `sessions::{SessionKind::{Panel,MainTab,GlobalView}, SessionBinding{plugin_key, kind, panel_id?, tab_id?, instance_id?, scope, repo_id?, worktree_id?, manifest_revision, owner: SessionOwner}, SessionRegistry::new(), issue(binding)->SessionToken(32B base64url), resolve(token)->Option<SessionBinding>, invalidate_owner(owner), invalidate_plugin(plugin_key), sweep_stale()`}`。
- [ ] **Step 1: 写失败测试**：签发/解析 round-trip；owner 失效；插件禁用→全部失效；revision 变化→绑定不匹配返回 None；token 唯一性与长度。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): 面板/主区 tab 会话注册表`

---

### Task 10: git runner + 市场

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/git.rs`
- Create: `src-tauri/crates/ade-plugins/src/marketplace/{mod.rs,store.rs,service.rs,installer.rs}`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `git::{GitRunner::run(args, timeout, cancel) -> Output, clone_depth1(url, ref, dest), fetch_commit(url, sha, dest), resolve_head(dir), ls_remote(url, ref)->Option<sha>}`（argv、无 shell、超时 kill+reap，仿 `ade-git/src/runner.rs`；URL 过 `is_plugin_git_url`）。
  - `marketplace::store::{MarketplaceStore::new(data_dir), list_sources, add_source{source_id,url,ref}, remove_source, read_snapshot/write_snapshot}`（`plugins-data/marketplaces/`）。
  - `marketplace::service::{MarketplaceService::refresh(source_id?), list_plugins()->Vec<Listing>, 官方种子 `https://github.com/stablyai/orca-plugins.git#main`, official trust 三重校验照 `orca:src/main/plugins/plugin-marketplace-provenance.ts`}`。
  - `marketplace::installer::{preview(source_id, plugin_key, git)->Preview{resolved_commit,content_hash,consent_fingerprint,manifest_projection}, install(preview, scope_ctx)->InstallResult(commit 未变校验), rollback(key, roots)}`。
- [ ] **Step 1: 写失败测试**（本地临时 git 仓库 ×2：marketplace 仓 + 插件仓，`git init` 脚本化）：add/refresh/snapshot 解析；preview 固定 commit；install 后 commit 漂移→拒绝；rollback；official trust 负例（非 github.com/stablyai）。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): git runner 与 git 源市场`

---

### Task 11: watcher + 事件 + service 编排

**Files:**
- Create: `src-tauri/crates/ade-plugins/src/watcher.rs`
- Create: `src-tauri/crates/ade-plugins/src/events.rs`
- Create: `src-tauri/crates/ade-plugins/src/service.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces：
  - `watcher::{DevWatcher::new(paths, batch_ms), start(cb), stop()}`（notify，150/500ms 批处理，仿 `ade-fs/src/watch.rs`）。
  - `events::{PluginEventName（3）, WorktreeCreated/Removed/AgentStatusChanged payload struct（照 `orca:src/shared/plugins/plugin-events.ts`），project_payload(name,&plugin_value)-> Option<Value>}`。
  - `service::{PluginService::new(config: ServiceConfig{app_data_dir, host_entry_path, node_override, event_sink, consent_state, secrets, storage, audit, host_services: Arc<dyn PluginHostServices>}), start(), shutdown(), list(scope_ctx) -> Vec<PluginListEntry>, install(source, scope_ctx), remove(ref), consent(req), set_enabled(ref), invoke_command(ref,command_id,args), panel_entry(ref,panel_id)->PanelEntry{html,session_token}, panel_action(token,action,params)->Outcome, global_view_entry(key)->PanelEntry, get_logs(key)->Vec<LogLine>, refresh(), lockfile_state(repo_id)->LockState, install_missing_from_lockfile(repo_id)->Vec<InstallResult>, on_worktree_created/removed(payload), on_agent_status_changed(payload), open_tab_limits 常量}`。
  - 激活协调：全局启动加载 enabled；`set_projects(projects: Vec<(repo_id, path)>)` 重算项目作用域加载/终止；lazy 激活（commands/events/ensure）；idle reap tick。
  - `PluginListEntry`（对齐 TS `PluginHostListEntry` 字段，含 scope/repo_id/tabs/status/needs_reconsent/capabilities/panels/commands/keybindings/source…）。
- [ ] **Step 1: 写失败测试**（fake host services + fake node）：list 投影（遮罩/状态）；启用→worker 激活→invoke 全链路；事件源→订阅投递；projects 变更加载/终止；lockfile_state/批量安装（临时插件 + lockfile fixture）；open_tab 回调限额/去重。
- [ ] **Step 2: RED → 实现 → GREEN**；Run: `cargo test -p ade-plugins`
- [ ] **Step 3: 提交** `feat(plugins): dev watcher、插件事件与 service 编排`

---

### Task 12: bridge 命令 + AppState + 事件 + bindings

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/plugins_service.rs`（PluginService 组装 + `PluginHostServices` 实现 + `ConsentState`/secrets 适配 + `ServiceEventSink`）
- Create: `src-tauri/crates/ade-bridge/src/commands/plugins.rs`（~22 命令 + specta DTO）
- Modify: `src-tauri/crates/ade-bridge/src/{state.rs, commands/mod.rs, specta_export.rs, lib.rs, events.rs}`
- Modify: `src-tauri/src/lib.rs`（Exit teardown）
- Create: `src-tauri/crates/ade-bridge/tests/plugins.rs`

**Interfaces:**
- Consumes：`ade-plugins::service::{PluginService, ServiceConfig, PluginListEntry, LockState}` 等（T1–T11）。
- Produces：
  - `AppState.plugins: Arc<PluginService>`；`initialize` 组装（app_data_dir、`resource_dir()/plugins/host-entry.mjs`、`ORCINUS_NODE_PATH`、settings 的 consent/disable/`devPluginPaths`/`pluginSystemEnabled`、pty host 终端查询、notifications 通道、ui state 活动 worktree、ProjectsStore 项目集合→`set_projects`）。
  - 命令（snake_case，camelCase args，DTO 对齐 TS 契约）：`plugins_list/plugins_list_language_packs/plugins_consent/plugins_set_enabled/plugins_read_panel_entry/plugins_invoke_command/plugins_panel_action/plugins_install/plugins_list_marketplaces/plugins_add_marketplace/plugins_remove_marketplace/plugins_refresh_marketplaces/plugins_list_marketplace_plugins/plugins_preview_marketplace_plugin/plugins_install_marketplace_plugin/plugins_preview_marketplace_update/plugins_rollback_marketplace_plugin/plugins_remove/plugins_get_logs/plugins_refresh/plugins_get_project_lock_state/plugins_install_missing_from_lockfile`。
  - 事件：`plugins:changed`（`{contentPacksChanged:bool, pluginsChanged:bool}`）与 **`plugins:open-tab`**（`{pluginKey,tabId,instanceId,title?,params,worktreeId,scope,repoId?}`——renderer 收到后创建主区 tab；去重已在 Rust 侧判定并返回既有 instanceId）。
  - `workspace.openTab` 的 services 回调：生成 instanceId、判定去重、emit `plugins:open-tab`、返回 `{instanceId, worktreeId}`。
  - bindings 重生成 + `bindings_are_fresh`；`tests/plugins.rs`（state-free `*_impl` + tempdir + fake node 脚本）：list/install/consent/set_enabled/panel_entry/panel_action/invoke/lock 状态/命令清单。
- [ ] **Step 1: 测试先行（bridge 集成）**；`cargo test -p ade-bridge`
- [ ] **Step 2: 提交** `feat(bridge): 插件服务装配、22 命令与事件`

---

### Task 13: TS 契约扩展 + mock + parity

**Files:**
- Modify: `src/shared/preload-api/api/plugin-host-api.ts`（`scope/repo_id`、`tabs`、`PluginRef`、install scope、lock 类型、panel action 集合 + `workspace.openTab`、capability 联合 + `workspace:open-tab`、`onOpenTab`）
- Modify: `src/shared/plugins/{plugin-capabilities.ts, plugin-manifest.ts(类型侧), plugin-panel-bridge.ts, plugin-install-lockfile.ts(类型侧)}`（如需）
- Modify: `src/bridge/mock/plugins-api.ts` + `src/bridge/mock/fixtures.ts`（scope 真字段、tabs、lock、onOpenTab）
- Modify: 相关 shared/mock 测试

**Interfaces:**
- Produces（渲染层后续任务消费的契约）：
  - `PluginHostListEntry` 增 `scope:'global'|'project'`、`repoId?: string`、`tabs: PluginHostTab[]`；`PluginRef{pluginKey,scope,repoId?}`；**`consent/setEnabled/remove/readPanelEntry/invokeCommand` 入参带 `PluginRef`**（同一 key 双作用域时定位 worker/实例）；`PluginHostInstallSource` 增 `{scope, repoId?}`。
  - 新类型：`PluginProjectLockState{repoId,lockfilePath,entries:[{pluginKey,version,source,contentHash,status:'installed'|'missing'|'hash-mismatch'|'source-unavailable',detail?}]}`；`PluginsApi` 增 `getProjectLockState({repoId})`、`installMissingFromLockfile({repoId})`、`onOpenTab(cb)=>()=>void`；`PluginOpenTabRequest`。
  - mock fixtures：scope 真字段（删 SAFETY cast 注释）；mock 实现新方法（lock 空态、onOpenTab no-op）。
- [ ] **Step 1: 契约/测试先行**：更新契约 + mock + `boot-namespaces`/`plugins-api.test`；`pnpm vitest run src/bridge src/shared/plugins`
- [ ] **Step 2: 提交** `feat(plugins): 契约扩展（scope/tabs/lock/open-tab）与 mock 同步`

---

### Task 14: `real/plugins.ts` + create-api + parity

**Files:**
- Create: `src/bridge/real/plugins.ts`
- Modify: `src/bridge/create-api.ts`（`RealDomains` 加 `'plugins'`；mock 保留 fixtures 供 mock 模式）
- Modify: `src/bridge/real/parity.test.ts`、`create-api.test.ts`
- Create: `src/bridge/real/plugins.test.ts`

**Interfaces:**
- Consumes：T12 命令名 + T13 契约。
- Produces：`createPluginsRealApi()` 全 23 方法（21 + lock 2）经 `invokeCommand`；`onChanged`/`onOpenTab` 订阅 Tauri 事件（`subscribeToEvent`），返回退订；panel action 方法名/参数精确。
- [ ] **Step 1: 测试先行**（路由断言 + 事件订阅/退订 + options 透传）；`pnpm vitest run src/bridge`
- [ ] **Step 2: 提交** `feat(plugins): real 域接线与 parity`

---

### Task 15: 渲染层 — scope 真数据 + 安装作用域 + consent 适配

**Files:**
- Modify: `src/renderer/src/store/slices/plugin-center/plugin-center-slice.ts`（类型自 shared）
- Modify: `src/renderer/src/components/settings/{PluginInstallDialog.tsx, plugin-install-source.ts, PluginConsentDialog.tsx}`、`PluginCenterEntryRow.tsx`（真 scope 徽标）
- Modify: `src/renderer/src/components/right-sidebar/plugin-panel-activity-items.ts`（真 scope+repo 过滤）
- Modify: 相应测试

**Interfaces:**
- Produces：安装源解析带 `scope/repoId`（`plugin-install-source.ts`）；对话框作用域单选（global 默认；无活动项目时 project 禁用并解释）；consent 对话框展示 scope 与项目不可信提示；行徽标/右侧栏过滤用真字段。
- [ ] **Step 1: 测试先行（组件/解析器）**；`pnpm vitest run src/renderer/src/components/settings src/renderer/src/components/right-sidebar src/renderer/src/store`
- [ ] **Step 2: 提交** `feat(plugins-ui): 作用域真数据、安装选择与 consent 适配`

---

### Task 16: 渲染层 — lockfile 提示 + 全局插件视图

**Files:**
- Create: `src/renderer/src/components/plugin-center/ProjectLockfileNotice.tsx`（banner + 弹层）
- Modify: `src/renderer/src/components/plugin-center/PluginCenterPage.tsx`（installed tab 顶部挂载）
- Modify: `src/renderer/src/components/global-rail/GlobalPluginHostView.tsx`（接真：读 global view entry + iframe shell 复用面板桥）
- Create/Modify: 相应测试

**Interfaces:**
- Produces：活动项目存在时调 `getProjectLockState`；`installed/missing/hash-mismatch/source-unavailable` 逐条渲染 + 「安装缺失」→ `installMissingFromLockfile` → `onChanged` 刷新；无缺失不渲染。`GlobalPluginHostView` 复用 `PluginPanel` 的 shell/桥：`readPanelEntry({pluginKey, panelId: <首个 panel>, scope:'global'})`；无 panel 贡献 → 不可用态（多 panel 全局插件的 rail 选择器为后续项，记录）。
- [ ] **Step 1: 测试先行（banner 状态矩阵/批量动作/视图加载）**；`pnpm vitest run`
- [ ] **Step 2: 提交** `feat(plugins-ui): lockfile 提示与全局插件视图接真`

---

### Task 17: 渲染层 — 主区插件 Tabs

**Files:**
- Modify: `src/shared/tab-types.ts`（`TabContentType`/`WorkspaceVisibleTabType` + `'plugin'`；`Tab.pluginTab?`）
- Modify: `src/renderer/src/store/slices/tabs/*`（创建/关闭/水合/最近关闭兼容）
- Create: `src/renderer/src/components/tabs/PluginTabContent.tsx`（iframe shell + `orca-panel-init`）
- Modify: tab bar 创建入口（`TabBarCreateEntry.tsx` 等「插件 Tab」分组）
- Modify: 主区 tab 内容 switch（定位现有 content renderer）；`src/renderer/src/store` 订阅 `onOpenTab`
- Create/Modify: 相应测试

**Interfaces:**
- Produces：`openPluginTab({pluginKey,tabId,scope,repoId?,instanceId,title?,params,worktreeId})` store action；`onOpenTab` 事件→store action（instanceId 去重）；「+」菜单列出当前 worktree 可用插件 tabs；`PluginTabContent` 会话 token + init 参数下发；插件被禁用/不可用→tab 不可用态可关闭；水合容错（旧状态无字段照常）。
- [ ] **Step 1: 测试先行（模型/水合/菜单/init 消息）**；`pnpm vitest run src/renderer/src/store/slices/tabs src/renderer/src/components/tab-bar`
- [ ] **Step 2: 提交** `feat(plugins-ui): 主区插件 Tabs（模型/渲染/菜单/事件）`

---

### Task 18: 渲染层 — 市场 tab + 开发中 tab

**Files:**
- Modify: `src/renderer/src/components/plugin-center/PluginCenterPage.tsx`（market inline 组件接真：源列表/浏览/预览/安装/回滚）
- Modify: 开发中 tab（dev 插件列表 + 重载动作）+ 设置市场对话框（如未共用）
- Modify: 相应测试

**Interfaces:**
- Produces：市场 tab 调 `listMarketplaces/listMarketplacePlugins/previewMarketplacePlugin/installMarketplacePlugin/previewMarketplaceUpdate/rollbackMarketplacePlugin/refreshMarketplaces`；开发中 tab 调 `list`（dev 源）+ `refresh`。
- [ ] **Step 1: 测试先行**；`pnpm vitest run`
- [ ] **Step 2: 提交** `feat(plugins-ui): 市场与开发中 tab 接真`

---

### Task 19: 门禁复跑 + 收尾记录

**Files:**
- Create: `docs/phase3a-plugin-system-record.md`
- Modify: `docs/superpowers/plans/2026-10-11-plugin-system.md`（勾选）

**Interfaces:**
- Consumes：spec §6/§7。
- Produces：记录（镜像 `docs/phase2g-computer-use-record.md`：§1 范围/§2 提交清单 `git log --oneline --reverse a2c8e021..HEAD`/§3 三门禁证据/§4 手工验收 8 项待复核/§5 偏差备案（spec §7 十二项 + 实施新发现）/§6 后续）。

- [ ] **Step 1: 三门禁 + bindings**
```bash
# workdir src-tauri
cargo test --workspace
cargo run -p ade-bridge --bin export-bindings && git diff --exit-code src/bridge/real/generated/tauri-bindings.ts
# repo root
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
pnpm test
```
- [ ] **Step 2: 记录 + 勾选 + 提交** `docs: Phase 3A 插件系统实施记录与门禁证据`

---

## 附：任务依赖

```
T1 → T2 → T3 → T4 → T5 ─┐
T6 → T7 → T8 → T9 ──────┤
T10 ────────────────────┼→ T11 → T12 → T14 → T15 → T16 → T17 → T18 → T19
T13（契约）──────────────┘   （T13 可与 T1–T12 并行；T14 依赖 T12+T13）
```
（T1–T5 纯逻辑串行；T6–T9 worker/host；T10 市场；T11 编排；T12 bridge；T13 契约；T14 real 域；T15–T18 渲染层；T19 收尾。全部在 main 直接提交。）
