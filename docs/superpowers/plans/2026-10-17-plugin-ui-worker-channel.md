# Phase 3C 插件 UI↔worker 通道与 updater 插件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Phase 3A 插件系统上补全 UI↔worker 双向通道（`worker.invokeCommand` + `session.post` + 命令可见性），并以 `plugins/updater` 插件（npm 全局 + Homebrew 更新管理，panel + tab）作为首个真实消费者。

**Architecture:** 平台面全部 additive：能力闭集 8→10、host methods 14→16、`invokeCommand` 帧可选 `session`、新 Tauri 事件 `plugins:panel-push` 经既有 `ServiceEventSink` seam 发射、渲染层在共用 hook `use-plugin-panel-entry.ts` 内按 token 转发。插件面：worker（TS→esbuild→`dist/main.mjs`，管理器适配器注册表 + 单飞任务）与 UI（React 19 + Tailwind v4→vite 多入口→单文件内联 HTML），全部决定论可测。

**Tech Stack:** Rust（ade-plugins / ade-bridge）、serde_json、Tauri 2、React 19、Tailwind v4、Vite、esbuild、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-17-plugin-ui-worker-channel-design.md`（实现依据；冲突以 spec 为准并在实施记录备案）

## Global Constraints

以下约束适用于每一个任务，取自 spec 原文：

- 能力闭集 8→10：新增 `worker:call`、`session:push`；仅声明它们的插件 consent 指纹受影响。
- host methods 14→16：`worker.invokeCommand`（仅 panel_callable=true）、`session.post`（仅 worker；panel 调用得 `panel_forbidden`）。
- 预算（每 token 每方向独立）：64KiB / 30 条每 10s；超限 `rate_limited`。
- 尺寸上限：`commandId` ≤256 UTF-16；`args` / handler 结果 / `payload` JSON 序列化 ≤64KiB（深度 ≤64）；`topic` ≤64；`sessionToken` 32..128。
- 命令可见性：`CommandContrib.visibility: "palette" | "internal"`，缺省 `"palette"`（additive）；internal 不进命令面板/快捷键面、禁 `action` 别名、keybinding 引用 internal → manifest 校验失败。
- 错误映射：未声明命令在面板层翻译为 `invalid_params`（服务层 `unknown_command`）；worker 未激活/errored → `unavailable`；崩溃/30s 超时/handler 抛错 → `action_failed`；结果超限 → `invalid_params`；`session.post` token 无效/非本插件/非 UI kind → `unavailable`；panel 调 `session.post` → `panel_forbidden`。
- updater 插件：id `updater`、publisher `itsuka`、version `0.1.0`、`engines.orca: ">=0.0.1"`、pluginApi 1；capabilities = `worker:call` / `session:push` / `workspace:open-tab` / `storage`；命令 `check` / `update-all` / `cancel` / `open`（palette）+ `snapshot` / `set-managers`（internal）。
- 更新语义：npm 全局追 latest（`npm install -g <name>@latest`）、brew `brew upgrade <name>` / `brew upgrade --cask <name>`；**逐包**执行、单包失败不阻断、取消杀当前子进程（SIGTERM→2s→SIGKILL）、剩余标 skipped。
- 流控：日志行缓冲 ≤2Hz 刷、单行截断 4KB、单推 ≤64KiB。
- 契约成员 25→26（23 命令 + 3 订阅：`onChanged` / `onOpenTab` / `onPanelPush`）；bindings 因 `PluginCommandDto` 增 `visibility` 需重生成。
- UI（六项一致性约束）：React 19 + Tailwind v4 同版本；只消费 shell 注入的 20 个 CSS var；`@theme inline` 映射块与宿主 `src/renderer/src/assets/main.css` 逐字相同（parity 测试锁死）；`@custom-variant dark (&:is(.dark *))` 逐字复制；Geist 68KB 内联为 data: URI；不引入 daisyUI；英文文案。
- 构建：`plugins/updater/dist/` 不入库（`.gitignore`）；CSP 只允许内联 script/style 与 data: 字体，产物必须是单文件 HTML。
- 平台：POSIX/macOS（沿 Phase 3A 现状）；系统 `node >= 18`。

**提交规范**：沿用仓库惯例 `type(scope): 描述（TN）`，例如 `feat(plugins): 新增 worker:call/session:push 能力（T1）`。每个任务最后一步提交。

**执行注意**：T4 与 T5 依次编辑同一批文件（`host/services.rs`/`host/methods.rs`），必须按顺序执行；每任务先跑该任务测试取得快速反馈，终结态再跑全量门禁（T18）。

---

# A 部分：平台通道（ade-plugins / ade-bridge / TS 契约）

## Task 1: capability 闭集新增 `worker:call` / `session:push`

**Files:**
- Modify: `src-tauri/crates/ade-plugins/src/types.rs`（enum / ALL / as_str；文件末尾若无测试模块则新增）
- Modify: `src-tauri/resources/plugins/host-entry.mjs:22-31`（`PLUGIN_CAPABILITY_KINDS`）
- Modify: `src-tauri/crates/ade-plugins/src/manifest.rs`（tests 模块，~L790 起）
- Modify: `src-tauri/crates/ade-plugins/tests/host_entry.rs`

**Interfaces:**
- Produces: `Capability::WorkerCall` / `Capability::SessionPush`（`as_str()` = `"worker:call"` / `"session:push"`），供 T3/T4/T5/T7 使用。

- [ ] **Step 1: 写失败测试（types.rs 末尾新增测试模块）**

```rust
#[cfg(test)]
mod tests {
    use super::Capability;

    #[test]
    fn capability_kinds_round_trip_all_ten() {
        assert_eq!(Capability::ALL.len(), 10);
        for capability in Capability::ALL {
            assert_eq!(Capability::from_kind(capability.as_str()), Some(capability));
        }
        assert_eq!(Capability::WorkerCall.as_str(), "worker:call");
        assert_eq!(Capability::SessionPush.as_str(), "session:push");
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-plugins capability_kinds_round_trip_all_ten`
Expected: 编译失败（`no variant named WorkerCall`）

- [ ] **Step 3: 实现（types.rs）**

- enum 插入两个变体（保持既有的字母序）：`Secrets,` 之后 `SessionPush,`；`TerminalSend,` 之后 `WorkerCall,`。
- `ALL` 改为 `[Capability; 10]`，按同序插入 `Capability::SessionPush,`（Secrets 后）与 `Capability::WorkerCall,`（TerminalSend 后）。
- `as_str` 增两臂：`Capability::SessionPush => "session:push",`、`Capability::WorkerCall => "worker:call",`。

- [ ] **Step 4: host-entry.mjs 同步闭集（L22-31）**

```js
const PLUGIN_CAPABILITY_KINDS = [
  'workspace:read',
  'terminal:send',
  'notifications:show',
  'storage',
  'secrets',
  'events:subscribe',
  'settings:own',
  'workspace:open-tab',
  'session:push',
  'worker:call'
]
```

- [ ] **Step 5: manifest 解析测试（manifest.rs tests 模块，使用既有 `base_manifest()` / `assert_invalid()`）**

```rust
#[test]
fn accepts_channel_capability_kinds_and_rejects_unknown() {
    let mut value = base_manifest();
    value["capabilities"] = json!([{ "kind": "worker:call" }, { "kind": "session:push" }]);
    let manifest = parse(value).expect("channel capabilities are valid");
    assert!(manifest.capabilities.contains(&Capability::WorkerCall));
    assert!(manifest.capabilities.contains(&Capability::SessionPush));

    let mut unknown = base_manifest();
    unknown["capabilities"] = json!([{ "kind": "ui:call" }]);
    assert_invalid(unknown, "capabilities");
}
```

- [ ] **Step 6: host_entry.rs 集成测试**

复制同文件 `hello_plugin_worker_round_trip_over_ndjson`（L186 起）的 tempdir/fixture/harness 搭建方式（node 缺失时按既有 SKIP 模式直接 return），新增：

```rust
#[test]
fn init_accepts_the_channel_capability_kinds() {
    let Some(node) = node_executable() else { return };
    let temp = /* 按既有测试的临时目录方式 */;
    let plugin_root = write_fixture(&temp, "export default function activate() {}\n");
    let mut harness = WorkerHarness::start(&node, &plugin_root);
    let mut init = init_frame(&plugin_root);
    if let ParentFrame::Init { granted_capabilities, .. } = &mut init {
        *granted_capabilities = vec!["worker:call".to_owned(), "session:push".to_owned()];
    }
    harness.send(&init);
    match harness.next_frame() {
        ChildFrame::Ready { .. } => {}
        other => panic!("unexpected frame: {other:?}"),
    }
}
```

（`ParentFrame` / `ChildFrame` / `init_frame` / `write_fixture` / `WorkerHarness` 均为该文件既有导入与工具，按邻近测试校准签名。）

- [ ] **Step 7: 运行全部相关测试**

Run: `cargo test -p ade-plugins capability` 与 `cargo test -p ade-plugins --test host_entry`
Expected: PASS（node 存在时实跑）

- [ ] **Step 8: 提交**

```bash
git add src-tauri/crates/ade-plugins/src/types.rs src-tauri/resources/plugins/host-entry.mjs src-tauri/crates/ade-plugins/src/manifest.rs src-tauri/crates/ade-plugins/tests/host_entry.rs
git commit -m "feat(plugins): 新增 worker:call/session:push 能力（T1）"
```

## Task 2: `CommandContrib.visibility`（manifest + 投影）

**Files:**
- Modify: `src-tauri/crates/ade-plugins/src/manifest.rs`（结构体 + 校验 + tests）
- Modify: `src-tauri/crates/ade-plugins/src/service.rs`（`PluginCommandProjection` + `project_entry` 映射 + tests）

**Interfaces:**
- Produces: `crate::manifest::CommandVisibility { Palette, Internal }`（`Default = Palette`，serde `lowercase`）；`PluginCommandProjection.visibility: CommandVisibility`。供 T6（bridge DTO）与 T9（TS 过滤）使用。

- [ ] **Step 1: 写失败测试（manifest.rs tests）**

```rust
#[test]
fn command_visibility_defaults_to_palette_and_rejects_conflicts() {
    let mut value = base_manifest();
    value["contributes"]["commands"] = json!([{ "id": "check", "title": "Check" }]);
    let manifest = parse(value).expect("defaults to palette");
    assert_eq!(
        manifest.contributes.commands[0].visibility,
        CommandVisibility::Palette
    );

    let mut internal = base_manifest();
    internal["contributes"]["commands"] =
        json!([{ "id": "snapshot", "title": "Snapshot", "visibility": "internal" }]);
    let manifest = parse(internal).expect("internal is valid");
    assert_eq!(
        manifest.contributes.commands[0].visibility,
        CommandVisibility::Internal
    );

    let mut with_action = base_manifest();
    with_action["contributes"]["commands"] = json!([{
        "id": "snapshot", "title": "Snapshot", "visibility": "internal", "action": "open-tab"
    }]);
    assert_invalid(with_action, "must not declare an action alias");

    let mut bound = base_manifest();
    bound["contributes"]["commands"] =
        json!([{ "id": "snapshot", "title": "Snapshot", "visibility": "internal" }]);
    bound["contributes"]["keybindings"] = json!([{ "command": "snapshot", "key": "Cmd+Shift+U" }]);
    assert_invalid(bound, "internal commands cannot be bound");

    let mut unknown = base_manifest();
    unknown["contributes"]["commands"] =
        json!([{ "id": "check", "title": "Check", "visibility": "hidden" }]);
    assert_invalid(unknown, "visibility");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-plugins command_visibility_defaults_to_palette_and_rejects_conflicts`
Expected: FAIL（无 `visibility` 字段 / 无 `CommandVisibility`）

- [ ] **Step 3: 实现（manifest.rs）**

结构体（`CommandContrib` 上方新增 enum；`CommandContrib` 增字段）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommandVisibility {
    #[default]
    Palette,
    Internal,
}
```

```rust
pub struct CommandContrib {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<CommandContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default)]
    pub visibility: CommandVisibility,
}
```

命令校验循环（`contributes.commands` 的 for 内，action 校验之后）：

```rust
if command.visibility == CommandVisibility::Internal && command.action.is_some() {
    return Err(invalid(format!(
        "contributes.commands.{index}: internal commands must not declare an action alias"
    )));
}
```

keybinding 校验循环（找到 `command` 之后、context 校验之前）：

```rust
if command.visibility == CommandVisibility::Internal {
    return Err(invalid(format!(
        "contributes.keybindings.{index}.command: internal commands cannot be bound"
    )));
}
```

- [ ] **Step 4: 投影（service.rs）**

`PluginCommandProjection`（L275）增字段：

```rust
pub struct PluginCommandProjection {
    pub id: String,
    pub title: String,
    pub context: crate::manifest::CommandContext,
    pub handler: PluginCommandHandler,
    pub keybindings: Vec<PluginKeybindingProjection>,
    pub visibility: crate::manifest::CommandVisibility,
}
```

`project_entry`（~L2167）中命令映射处（`PluginCommandProjection {` 构造字面量）增 `visibility: command.visibility,`（`command` 为该处循环变量；编译器会指出确切位置）。

- [ ] **Step 5: service.rs 投影测试**

在 service.rs tests 模块中、改造/新增一个最小投影断言（沿用该模块既有构建 fixture 的辅助函数；若既有测试已断言 commands 投影，直接在其上追加 `visibility: CommandVisibility::Internal` 用例）：

```rust
// 断言示例（按既有夹具变量名调整）：
let entry = /* 构建含 internal 命令的插件并投影 */;
let command = &entry.commands[0];
assert_eq!(command.visibility, crate::manifest::CommandVisibility::Internal);
```

- [ ] **Step 6: 运行**

Run: `cargo test -p ade-plugins command_visibility` 与 `cargo test -p ade-plugins project`
Expected: PASS

- [ ] **Step 7: 提交**

```bash
git add src-tauri/crates/ade-plugins/src/manifest.rs src-tauri/crates/ade-plugins/src/service.rs
git commit -m "feat(plugins): 命令可见性 visibility（manifest+投影）（T2）"
```

## Task 3: host methods 14→16（api 表 + 校验器 + gate 矩阵）

**Files:**
- Modify: `src-tauri/crates/ade-plugins/src/host/api.rs`
- Modify: `src-tauri/crates/ade-plugins/src/host/gate.rs`

**Interfaces:**
- Consumes: T1 的 `Capability::WorkerCall` / `Capability::SessionPush`。
- Produces: `api::spec("worker.invokeCommand")` / `api::spec("session.post")`；常量 `WORKER_COMMAND_ID_MAX_CHARS`、`WORKER_COMMAND_RESULT_MAX_BYTES`、`SESSION_POST_TOPIC_MAX_CHARS`、`SESSION_POST_PAYLOAD_MAX_BYTES`。供 T4/T5/T7。

- [ ] **Step 1: 写失败测试（api.rs tests 与 gate.rs tests）**

api.rs tests 内：把既有 `spec_table_has_fourteen_methods_with_expected_metadata` 改名/更新为：

```rust
#[test]
fn spec_table_has_sixteen_methods_with_expected_metadata() {
    assert_eq!(specs().len(), 16);
    let names: Vec<&str> = specs().iter().map(|entry| entry.name).collect();
    assert_eq!(
        names,
        vec![
            "workspace.readContext",
            "workspace.openTab",
            "worker.invokeCommand",
            "terminal.sendText",
            "notifications.show",
            "storage.get",
            "storage.set",
            "storage.delete",
            "storage.keys",
            "secrets.get",
            "secrets.set",
            "secrets.delete",
            "settings.get",
            "settings.set",
            "events.subscribe",
            "session.post",
        ]
    );
    let panel: Vec<&str> = specs()
        .iter()
        .filter(|entry| entry.panel_callable)
        .map(|entry| entry.name)
        .collect();
    assert_eq!(
        panel,
        vec![
            "workspace.readContext",
            "workspace.openTab",
            "worker.invokeCommand",
            "terminal.sendText",
            "notifications.show",
        ]
    );
    assert_eq!(
        spec("worker.invokeCommand").unwrap().capability,
        Capability::WorkerCall
    );
    assert_eq!(spec("session.post").unwrap().capability, Capability::SessionPush);
    assert!(!spec("worker.invokeCommand").unwrap().mutation);
    assert!(!spec("session.post").unwrap().mutation);
}

#[test]
fn worker_invoke_and_session_post_params_enforce_bounds() {
    assert_eq!(
        validate_params("worker.invokeCommand", json!({ "commandId": "check" })).unwrap(),
        json!({ "commandId": "check" })
    );
    assert!(validate_params("worker.invokeCommand", json!({})).is_err());
    assert!(validate_params("worker.invokeCommand", json!({ "commandId": "" })).is_err());
    assert!(validate_params(
        "worker.invokeCommand",
        json!({ "commandId": "c".repeat(WORKER_COMMAND_ID_MAX_CHARS + 1) })
    )
    .is_err());
    let big_args = "x".repeat(64 * 1024);
    assert!(validate_params(
        "worker.invokeCommand",
        json!({ "commandId": "check", "args": { "blob": big_args } })
    )
    .is_err());

    let token = "t".repeat(32);
    assert_eq!(
        validate_params(
            "session.post",
            json!({ "sessionToken": token, "topic": "log", "payload": { "line": "x" } })
        )
        .unwrap(),
        json!({ "sessionToken": "t".repeat(32), "topic": "log", "payload": { "line": "x" } })
    );
    assert!(validate_params("session.post", json!({ "sessionToken": "short", "payload": 1 })).is_err());
    assert!(validate_params("session.post", json!({ "sessionToken": token })).is_err());
    assert!(validate_params(
        "session.post",
        json!({ "sessionToken": token, "topic": "x".repeat(SESSION_POST_TOPIC_MAX_CHARS + 1), "payload": 1 })
    )
    .is_err());

    assert!(validate_result("worker.invokeCommand", json!({ "ok": true })).is_ok());
    assert!(validate_result("session.post", json!({ "ok": true })).is_ok());
    assert!(validate_result("session.post", json!({ "ok": false })).is_err());
    let huge = json!({ "blob": "x".repeat(WORKER_COMMAND_RESULT_MAX_BYTES + 1) });
    assert!(validate_result("worker.invokeCommand", huge).is_err());
}
```

gate.rs tests：`method_for` match 增 `Capability::WorkerCall => "worker.invokeCommand",` 与 `Capability::SessionPush => "session.post",`；`panel_surface_is_exactly_the_four_panel_callable_methods` 的 `panel_methods` 数组增 `"worker.invokeCommand"`，并把测试名与注释改为 "five panel callable methods"。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-plugins spec_table_has_sixteen_methods`
Expected: FAIL（`specs()` 仍是 14 / 找不到方法）

- [ ] **Step 3: 实现（api.rs）**

常量（顶部常量区）：

```rust
pub const WORKER_COMMAND_ID_MAX_CHARS: usize = 256;
pub const WORKER_COMMAND_RESULT_MAX_BYTES: usize = 64 * 1024;
pub const SESSION_POST_TOPIC_MAX_CHARS: usize = 64;
pub const SESSION_POST_PAYLOAD_MAX_BYTES: usize = 64 * 1024;
```

校验器（`check_json_bounds` 复用；新增）：

```rust
fn worker_invoke_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let command_id = require_string(obj, "commandId")?;
    non_empty(command_id, "commandId")?;
    max_utf16(command_id, WORKER_COMMAND_ID_MAX_CHARS, "commandId")?;
    let args = match obj.get("args") {
        None | Some(Value::Null) => None,
        Some(args) => {
            check_json_bounds(args, "args")?;
            Some(args.clone())
        }
    };
    let mut normalized = Map::new();
    normalized.insert("commandId".to_string(), Value::String(command_id.to_string()));
    if let Some(args) = args {
        normalized.insert("args".to_string(), args);
    }
    Ok(Value::Object(normalized))
}

fn worker_command_result(value: &Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| invalid("(root)", error))?;
    if bytes.len() > WORKER_COMMAND_RESULT_MAX_BYTES {
        return Err(invalid(
            "(root)",
            format!("must serialize to at most {WORKER_COMMAND_RESULT_MAX_BYTES} bytes"),
        ));
    }
    Ok(value.clone())
}

fn session_post_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let token = require_string(obj, "sessionToken")?;
    if token.len() < 32 || token.len() > 128 {
        return Err(invalid("sessionToken", "must contain between 32 and 128 characters"));
    }
    let topic = optional_string(obj, "topic")?;
    if let Some(topic) = topic {
        non_empty(topic, "topic")?;
        max_utf16(topic, SESSION_POST_TOPIC_MAX_CHARS, "topic")?;
    }
    let payload = obj
        .get("payload")
        .ok_or_else(|| invalid("payload", "required"))?
        .clone();
    let payload_bytes = serde_json::to_vec(&payload)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if payload_bytes > SESSION_POST_PAYLOAD_MAX_BYTES {
        return Err(invalid(
            "payload",
            format!("must serialize to at most {SESSION_POST_PAYLOAD_MAX_BYTES} bytes"),
        ));
    }
    let mut normalized = Map::new();
    normalized.insert("sessionToken".to_string(), Value::String(token.to_string()));
    if let Some(topic) = topic {
        normalized.insert("topic".to_string(), Value::String(topic.to_string()));
    }
    normalized.insert("payload".to_string(), payload);
    Ok(Value::Object(normalized))
}
```

表（`SPECS` 改 `[MethodSpec; 16]`；在 `workspace.openTab` 后插入 `worker.invokeCommand`，在 `events.subscribe` 后插入 `session.post`）：

```rust
MethodSpec {
    name: "worker.invokeCommand",
    capability: Capability::WorkerCall,
    mutation: false,
    panel_callable: true,
    params: worker_invoke_params,
    result: worker_command_result,
},
```

```rust
MethodSpec {
    name: "session.post",
    capability: Capability::SessionPush,
    mutation: false,
    panel_callable: false,
    params: session_post_params,
    result: ok_result,
},
```

`specs()` 返回类型改 `&'static [MethodSpec; 16]`。

- [ ] **Step 4: 运行**

Run: `cargo test -p ade-plugins --lib`（api/gate 全部）
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/ade-plugins/src/host/api.rs src-tauri/crates/ade-plugins/src/host/gate.rs
git commit -m "feat(plugins): host methods 扩至 16（worker.invokeCommand/session.post 表与门控）（T3）"
```

## Task 4: `session.post` 全链（ctx 会话 token + service + 事件 + bridge sink）

**Files:**
- Modify: `src-tauri/crates/ade-plugins/src/host/methods.rs`（ctx 字段、dispatch、handler、tests）
- Modify: `src-tauri/crates/ade-plugins/src/host/services.rs`（trait 增 `session_post`）
- Modify: `src-tauri/crates/ade-plugins/src/service.rs`（panel_action/WorkerBridge 构造点、`PushAdmission`、`PluginService::session_post`、`PanelPushRequest`、`ServiceEventSink`、tests 的 `FakeHostServices`/`RecordingSink`）
- Modify: `src-tauri/crates/ade-bridge/src/events.rs`（事件常量 + payload）
- Modify: `src-tauri/crates/ade-bridge/src/plugins_service.rs`（HostServices impl + `BridgeServiceEventSink`）
- Modify: `src-tauri/crates/ade-bridge/tests/plugins_service.rs`（`RecordingSink`）
- Test: 上述各文件内测试 + `src-tauri/crates/ade-bridge/tests/plugins_service.rs`

**Interfaces:**
- Produces: `HostCallContext.session_token: Option<&'a str>`；`PluginHostServices::session_post(plugin_key, session_token, topic, payload) -> Result<(), HostServiceError>`；`PluginService::session_post(...)`；`ServiceEventSink::panel_push_requested(PanelPushRequest)`；事件 `plugins:panel-push`。供 T5（trait 同文件追加）与 T7（TS 订阅）使用。

- [ ] **Step 1: 写失败测试（service.rs tests：校验/预算/发射）**

在 service.rs tests 模块新增（同模块可直接访问私有字段 `sessions`/`push_admission`）：

```rust
fn issue_ui_session(service: &PluginService, plugin_key: &str, kind: SessionKind) -> String {
    service.sessions.issue(SessionBinding {
        plugin_key: plugin_key.to_string(),
        kind,
        panel_id: Some("updater".to_string()),
        tab_id: None,
        instance_id: None,
        scope: PluginScope::Global,
        repo_id: None,
        worktree_id: None,
        manifest_revision: "rev".to_string(),
        owner: SessionOwner::Renderer {
            window_label: "main".to_string(),
        },
    })
}

#[test]
fn session_post_validates_binding_and_emits_pushes() {
    // 使用该模块既有构建 PluginService 的夹具（见 open_tab_dedupes_caps_rates_and_prunes 附近），
    // 事件 sink 换成记录型（RecordingSink 增加 pushes: Mutex<Vec<PanelPushRequest>>）。
    let (service, sink) = /* 既有夹具 */;
    let token = issue_ui_session(&service, "itsuka.updater", SessionKind::MainTab);

    service
        .session_post("itsuka.updater", &token, Some("state"), json!({ "n": 1 }))
        .expect("push accepted");
    assert_eq!(sink.pushes.lock().unwrap().len(), 1);

    assert_eq!(
        service
            .session_post("other.plugin", &token, None, json!({}))
            .unwrap_err()
            .code(),
        UNAVAILABLE
    );
    assert_eq!(
        service
            .session_post("itsuka.updater", "unknown-token", None, json!({}))
            .unwrap_err()
            .code(),
        UNAVAILABLE
    );
}

#[test]
fn session_post_budget_is_per_token() {
    let (service, _sink) = /* 同上夹具 */;
    let token = issue_ui_session(&service, "itsuka.updater", SessionKind::Panel);
    for _ in 0..PANEL_PUSH_MAX_PER_WINDOW {
        service
            .session_post("itsuka.updater", &token, None, json!({}))
            .expect("within budget");
    }
    let error = service
        .session_post("itsuka.updater", &token, None, json!({}))
        .unwrap_err();
    assert_eq!(error.code(), gate::RATE_LIMITED);

    let other = issue_ui_session(&service, "itsuka.updater", SessionKind::Panel);
    service
        .session_post("itsuka.updater", &other, None, json!({}))
        .expect("per-token buckets are independent");
}
```

methods.rs tests 新增 `session.post` 处理器测试：

```rust
#[test]
fn session_post_reaches_services_only_from_workers() {
    let services = services_with_context();
    let value = expect_ok(execute(
        "session.post",
        json!({ "sessionToken": "t".repeat(32), "topic": "log", "payload": { "line": "x" } }),
        &context(&services, None),
    ));
    assert_eq!(value, json!({ "ok": true }));
    assert_eq!(
        services.session_post_calls.lock().unwrap().as_slice(),
        [("t".repeat(32), Some("log".to_string()), json!({ "line": "x" }))]
    );

    let mut panel_ctx = context(&services, None);
    panel_ctx.via_panel = true;
    panel_ctx.session_token = Some("t".repeat(32).as_str());
    let (code, _) = expect_err(execute(
        "session.post",
        json!({ "sessionToken": "t".repeat(32), "payload": 1 }),
        &panel_ctx,
    ));
    assert_eq!(code, gate::PANEL_FORBIDDEN);
}
```

（`services_with_context()` 的 `FakeServices` 增字段 `session_post_calls: Mutex<Vec<(String, Option<String>, Value)>>` 并在 trait impl 中记录，返回 `Ok(())`。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-plugins session_post`
Expected: FAIL（字段/方法缺失）

- [ ] **Step 3: 实现（ade-plugins）**

methods.rs：
1. `HostCallContext` 增字段：

```rust
/// Panel-session token when the call arrives over a UI bridge; `None` for
/// worker-originated calls.
pub session_token: Option<&'a str>,
```
2. `dispatch` 增臂：`"session.post" => session_post(params, ctx),`
3. handler：

```rust
fn session_post(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let topic = params.get("topic").and_then(Value::as_str);
    let payload = field(params, "payload").clone();
    ctx.services
        .session_post(ctx.plugin_key, field_str(params, "sessionToken"), topic, payload)?;
    Ok(serde_json::json!({ "ok": true }))
}
```
4. 既有两个构造点补齐字段：`panel_action`（service.rs）设 `session_token: Some(token)`；`WorkerBridge::handle` 设 `None`；methods.rs tests 的 `context()` helper 设 `None`（并新增 `panel_ctx(services, audit, token)` helper 供新测试用）。

services.rs trait 增（无默认实现，强制所有 impl 表态）：

```rust
/// Push one message to a UI session of the calling plugin (spec §3.5).
fn session_post(
    &self,
    plugin_key: &str,
    session_token: &str,
    topic: Option<&str>,
    payload: Value,
) -> Result<(), HostServiceError>;
```

service.rs：
1. 常量与结构（`PanelAdmission` 旁）：

```rust
/// Push budget window per session token (spec §3.4).
pub const PANEL_PUSH_WINDOW: Duration = Duration::from_secs(10);
/// Maximum pushes per token inside [`PANEL_PUSH_WINDOW`].
pub const PANEL_PUSH_MAX_PER_WINDOW: usize = 30;
/// Maximum serialized push payload (spec §3.4: ≤64KiB).
pub const PANEL_PUSH_PAYLOAD_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Default)]
struct PushAdmission {
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl PushAdmission {
    fn admit(&self, token: &str, payload_bytes: usize) -> Result<(), PluginError> {
        if payload_bytes > PANEL_PUSH_PAYLOAD_MAX_BYTES {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("payload must not exceed {PANEL_PUSH_PAYLOAD_MAX_BYTES} bytes"),
            ));
        }
        let now = Instant::now();
        let mut hits = self
            .hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        hits.retain(|_, times| {
            while times
                .front()
                .is_some_and(|time| now.duration_since(*time) >= PANEL_PUSH_WINDOW)
            {
                times.pop_front();
            }
            !times.is_empty()
        });
        let times = hits.entry(token.to_string()).or_default();
        if times.len() >= PANEL_PUSH_MAX_PER_WINDOW {
            return Err(PluginError::new(
                gate::RATE_LIMITED,
                format!("push rate limit exceeded ({PANEL_PUSH_MAX_PER_WINDOW} per 10s)"),
            ));
        }
        times.push_back(now);
        Ok(())
    }
}
```
2. `PanelPushRequest`（`OpenTabRequest` 旁）：

```rust
/// Payload of the `plugins:panel-push` bridge event (spec §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelPushRequest {
    pub session_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub payload: Value,
}
```
3. `ServiceEventSink` 增 `fn panel_push_requested(&self, request: PanelPushRequest);`
4. `PluginService` 增字段 `push_admission: PushAdmission,`（`panel_admission` 后）与 `new()` 中 `push_admission: PushAdmission::default(),`
5. 方法（`panel_action` 后）：

```rust
pub fn session_post(
    &self,
    plugin_key: &str,
    session_token: &str,
    topic: Option<&str>,
    payload: Value,
) -> Result<(), PluginError> {
    let Some(binding) = self.sessions.resolve(session_token) else {
        return Err(PluginError::new(UNAVAILABLE, "session is not available"));
    };
    if binding.plugin_key != plugin_key {
        return Err(PluginError::new(
            UNAVAILABLE,
            "session does not belong to the calling plugin",
        ));
    }
    let payload_bytes = serde_json::to_vec(&payload)
        .map(|encoded| encoded.len())
        .unwrap_or(usize::MAX);
    self.push_admission.admit(session_token, payload_bytes)?;
    self.config
        .event_sink
        .panel_push_requested(PanelPushRequest {
            session_token: session_token.to_string(),
            topic: topic.map(str::to_string),
            payload,
        });
    Ok(())
}
```
6. tests：`FakeHostServices`（L2919）与 `RecordingSink`（L3025）补齐新 trait 方法（sink 记 `pushes: Mutex<Vec<PanelPushRequest>>`）。

- [ ] **Step 4: 实现（ade-bridge）**

events.rs（`PLUGINS_OPEN_TAB` 旁）：

```rust
pub const PLUGINS_PANEL_PUSH: &str = "plugins:panel-push";

/// Payload of [`PLUGINS_PANEL_PUSH`]（TS `PluginPanelPushRequest` 同形）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelPushPayload {
    pub session_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub payload: Value,
}

impl From<PanelPushRequest> for PluginPanelPushPayload {
    fn from(request: PanelPushRequest) -> Self {
        Self {
            session_token: request.session_token,
            topic: request.topic,
            payload: request.payload,
        }
    }
}
```

（`PanelPushRequest` 加入该文件的 use 列表。）

plugins_service.rs：
- `BridgePluginHostServices` impl 增：

```rust
fn session_post(
    &self,
    plugin_key: &str,
    session_token: &str,
    topic: Option<&str>,
    payload: Value,
) -> Result<(), HostServiceError> {
    let service = self
        .service
        .get()
        .ok_or_else(|| HostServiceError::new("unavailable", "plugin service is not assembled"))?;
    service
        .session_post(plugin_key, session_token, topic, payload)
        .map_err(|error| HostServiceError::new(error.code(), error.message()))
}
```
- `BridgeServiceEventSink` 增：

```rust
fn panel_push_requested(&self, request: PanelPushRequest) {
    let payload = events::PluginPanelPushPayload::from(request);
    self.emit_payload(
        events::PLUGINS_PANEL_PUSH,
        serde_json::to_value(payload).unwrap_or(Value::Null),
    );
}
```
- tests/plugins_service.rs 的 `RecordingSink` 增方法（记录到 `pushes`），并加一条断言测试：驱动 `BridgeServiceEventSink`/或直接调 sink 的 `panel_push_requested` 断言事件名与载荷 camelCase。

- [ ] **Step 5: 运行**

Run: `cargo test -p ade-plugins session_post` 与 `cargo test -p ade-bridge panel_push`
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/ade-plugins src-tauri/crates/ade-bridge
git commit -m "feat(plugins): session.post 全链（推送预算+事件 plugins:panel-push）（T4）"
```

## Task 5: `worker.invokeCommand` 全链（帧 session + SDK ctx + 服务透传 + 桥接）

**Files:**
- Modify: `src-tauri/crates/ade-plugins/src/worker/protocol.rs`（`SessionRef` + `ParentFrame::InvokeCommand.session` + tests）
- Modify: `src-tauri/crates/ade-plugins/src/worker/manager.rs`（`invoke_command` 透传 + tests 调用点）
- Modify: `src-tauri/crates/ade-plugins/src/service.rs`（`invoke_command_with_session`；既有 `invoke_command` 委托）
- Modify: `src-tauri/crates/ade-plugins/src/host/methods.rs`（`invoke_worker_command` handler + tests）
- Modify: `src-tauri/crates/ade-plugins/src/host/services.rs`（trait 增 `invoke_worker_command`）
- Modify: `src-tauri/crates/ade-bridge/src/plugins_service.rs`（HostServices impl）
- Modify: `src-tauri/crates/ade-plugins/src/service.rs` tests（`FakeHostServices`）
- Modify: `src-tauri/resources/plugins/host-entry.mjs`（帧解析 + `ctx.session`）
- Modify: `src-tauri/crates/ade-plugins/tests/host_entry.rs`

**Interfaces:**
- Consumes: T3 表与常量、T4 的 `HostCallContext.session_token`。
- Produces: `ParentFrame::InvokeCommand { call_id, command_id, args, session: Option<SessionRef> }`；SDK handler 第二参数 `ctx.session { token, post }`；`PluginHostServices::invoke_worker_command(...)`；`PluginService::invoke_command_with_session`。供 T7/T8 与 updater 使用。

- [ ] **Step 1: 写失败测试（protocol.rs tests）**

```rust
// parent_frames() 增一条带 session 的 InvokeCommand：
ParentFrame::InvokeCommand {
    call_id: 9,
    command_id: "check".to_owned(),
    args: None,
    session: Some(SessionRef { token: "t".repeat(32) }),
},

#[test]
fn invoke_command_session_is_optional_and_camel_case() {
    let without = ParentFrame::InvokeCommand {
        call_id: 8,
        command_id: "no-args".to_owned(),
        args: None,
        session: None,
    };
    let value = serde_json::to_value(&without).unwrap();
    assert!(value.get("session").is_none());

    let with = ParentFrame::InvokeCommand {
        call_id: 9,
        command_id: "check".to_owned(),
        args: None,
        session: Some(SessionRef { token: "t".repeat(32) }),
    };
    assert_eq!(
        serde_json::to_value(&with).unwrap()["session"],
        json!({ "token": "t".repeat(32) })
    );
}
```

（既有 `parent_frames()`/`parent_frame_shapes_match_oracle_fields`/round-trip 测试同步补 `session: None` 字段。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-plugins invoke_command_session`
Expected: FAIL（`SessionRef` 不存在）

- [ ] **Step 3: 实现 Rust 透传链**

protocol.rs：

```rust
/// Session identity attached to a UI-originated command invocation: the
/// worker can push progress back through `session.post`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub token: String,
}

// ParentFrame::InvokeCommand 增字段：
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<SessionRef>,
```

manager.rs `invoke_command` 增参 `session: Option<SessionRef>` 并透传到帧构造：

```rust
let reply = entry.supervisor.send(
    &ParentFrame::InvokeCommand {
        call_id,
        command_id: command_id.to_owned(),
        args,
        session,
    },
    ReplyKind::Command { call_id },
)?;
```

service.rs（`invoke_command` 改委托；新增）： 

```rust
pub fn invoke_command(
    &self,
    plugin_ref: &PluginRef,
    command_id: &str,
    args: Option<Value>,
) -> Result<Value, PluginError> {
    self.invoke_command_with_session(plugin_ref, command_id, args, None)
}

pub fn invoke_command_with_session(
    &self,
    plugin_ref: &PluginRef,
    command_id: &str,
    args: Option<Value>,
    session_token: Option<&str>,
) -> Result<Value, PluginError> {
    // ... 既有 invoke_command 全部逻辑 ...
    self.manager().invoke_command(
        &instance_key,
        call_id,
        command_id,
        args,
        session_token.map(|token| crate::worker::protocol::SessionRef {
            token: token.to_string(),
        }),
    )
}
```

（`SessionRef` 的模块路径按 `worker` 模块的 re-export 实际形状调整；`cargo check` 会指出。既有 manager/service 测试中 `invoke_command(...)` 直调点补 `None`。）

- [ ] **Step 4: methods.rs handler + trait + bridge impl**

services.rs trait 增：

```rust
/// Invoke a declared worker command on behalf of a UI session (spec §3.5);
/// `session_token` is attached to the worker frame for `session.post` routing.
fn invoke_worker_command(
    &self,
    plugin_key: &str,
    scope: PluginScope,
    repo_id: Option<&str>,
    command_id: &str,
    args: Option<Value>,
    session_token: Option<&str>,
) -> Result<Value, HostServiceError>;
```

methods.rs `dispatch` 增臂 `"worker.invokeCommand" => invoke_worker_command(params, ctx),`：

```rust
fn invoke_worker_command(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let Some(session_token) = ctx.session_token.filter(|_| ctx.via_panel) else {
        return Err(HandlerError::new(
            UNAVAILABLE,
            "worker.invokeCommand requires a UI session context",
        ));
    };
    let command_id = field_str(params, "commandId");
    let args = params.get("args").filter(|value| !value.is_null()).cloned();
    let value = ctx
        .services
        .invoke_worker_command(
            ctx.plugin_key,
            ctx.scope,
            ctx.repo_id,
            command_id,
            args,
            Some(session_token),
        )
        .map_err(|error| {
            if error.code() == UNKNOWN_COMMAND {
                HandlerError::new(INVALID_PARAMS, error.message().to_string())
            } else {
                error.into()
            }
        })?;
    let bytes = serde_json::to_vec(&value)
        .map_err(|error| HandlerError::new(INVALID_PARAMS, error.to_string()))?;
    if bytes.len() > api::WORKER_COMMAND_RESULT_MAX_BYTES {
        return Err(HandlerError::new(
            INVALID_PARAMS,
            format!(
                "command result exceeds {} bytes; use session.post to stream large payloads",
                api::WORKER_COMMAND_RESULT_MAX_BYTES
            ),
        ));
    }
    Ok(value)
}
```

（`UNKNOWN_COMMAND` 从 `crate::worker::manager::UNKNOWN_COMMAND` 导入；若模块私有则经 `crate::worker` re-export，按 `cargo check` 调整。）

bridge plugins_service.rs impl 增：

```rust
fn invoke_worker_command(
    &self,
    plugin_key: &str,
    scope: PluginScope,
    repo_id: Option<&str>,
    command_id: &str,
    args: Option<Value>,
    session_token: Option<&str>,
) -> Result<Value, HostServiceError> {
    let service = self
        .service
        .get()
        .ok_or_else(|| HostServiceError::new("unavailable", "plugin service is not assembled"))?;
    service
        .invoke_command_with_session(
            &PluginRef {
                plugin_key: plugin_key.to_string(),
                scope,
                repo_id: repo_id.map(str::to_string),
            },
            command_id,
            args,
            session_token,
        )
        .map_err(|error| HostServiceError::new(error.code(), error.message()))
}
```

methods.rs tests：`FakeServices` 增 `invoke_result: Mutex<Option<Result<Value, HostServiceError>>>` + `invoke_calls: Mutex<Vec<(String, Option<Value>, Option<String>)>>`；新增测试：

```rust
#[test]
fn worker_invoke_command_requires_a_panel_session_and_sanitizes_errors() {
    let services = services_with_context();
    // 面板会话上下文成功路径
    let token_owned = "t".repeat(32);
    let mut panel = context(&services, None);
    panel.via_panel = true;
    panel.session_token = Some(token_owned.as_str());
    *services.invoke_result.lock().unwrap() = Some(Ok(json!({ "started": true })));
    let value = expect_ok(execute(
        "worker.invokeCommand",
        json!({ "commandId": "check" }),
        &panel,
    ));
    assert_eq!(value, json!({ "started": true }));
    assert_eq!(services.invoke_calls.lock().unwrap().len(), 1);

    // worker 自调（无会话上下文）→ unavailable
    let (code, _) = expect_err(execute(
        "worker.invokeCommand",
        json!({ "commandId": "check" }),
        &context(&services, None),
    ));
    assert_eq!(code, UNAVAILABLE);

    // 未声明命令 → 面板层 invalid_params
    *services.invoke_result.lock().unwrap() =
        Some(Err(HostServiceError::new("unknown_command", "plugin does not declare command x")));
    let (code, error) = expect_err(execute(
        "worker.invokeCommand",
        json!({ "commandId": "missing" }),
        &panel,
    ));
    assert_eq!(code, INVALID_PARAMS);
    assert!(error.contains("does not declare"), "{error}");

    // 超限结果 → invalid_params
    *services.invoke_result.lock().unwrap() = Some(Ok(json!({ "blob": "x".repeat(64 * 1024 + 1) })));
    let (code, _) = expect_err(execute(
        "worker.invokeCommand",
        json!({ "commandId": "check" }),
        &panel,
    ));
    assert_eq!(code, INVALID_PARAMS);

    // 面板桥外（palette/worker）无 session → unavailable（同 worker 自调分支）
    let mut no_session = context(&services, None);
    no_session.via_panel = true;
    let (code, _) = expect_err(execute(
        "worker.invokeCommand",
        json!({ "commandId": "check" }),
        &no_session,
    ));
    assert_eq!(code, UNAVAILABLE);
}
```

- [ ] **Step 5: host-entry.mjs（帧解析 + SDK ctx）**

`parseParentFrame` 的 `invokeCommand` 分支追加可选 session 校验：

```js
    case 'invokeCommand':
      if (!isCallId(raw.callId) || !isNonEmptyString(raw.commandId)) return null
      if (raw.session !== undefined) {
        if (
          typeof raw.session !== 'object' ||
          raw.session === null ||
          !isNonEmptyString(raw.session.token) ||
          raw.session.token.length > 128
        ) {
          return null
        }
      }
      return raw
```

将既有 `orca.host.call` 内的 hostCall 发送抽成模块级 `hostCall(method, params)`（返回 Promise，`orca.host.call` 委托之），并新增：

```js
function makeCommandContext(message) {
  if (!message.session || typeof message.session.token !== 'string' || message.session.token.length === 0) {
    return undefined
  }
  const token = message.session.token
  return {
    session: {
      token,
      post(topic, payload) {
        return hostCall('session.post', { sessionToken: token, topic, payload })
      }
    }
  }
}
```

`handleInvokeCommand` 改为：

```js
async function handleInvokeCommand(message) {
  const handler = commandHandlers.get(message.commandId)
  // ... 既有 no-handler 分支不变 ...
  try {
    const value = await handler(message.args, makeCommandContext(message))
    send({ type: 'commandResult', callId: message.callId, ok: true, value })
  } catch (error) {
    // ... 既有错误分支不变 ...
  }
}
```

- [ ] **Step 6: host_entry.rs 集成测试**

```rust
#[test]
fn invoke_command_receives_session_context_and_posts_over_host_calls() {
    let Some(node) = node_executable() else { return };
    // 按既有测试方式建 tempdir 与 fixture（source 注册 echo 命令）：
    let source = r#"
export default function activate(orca) {
  orca.commands.register('probe', async (args, ctx) => {
    if (!ctx || !ctx.session) return { hasSession: false }
    await ctx.session.post('log', { line: 'hi' })
    return { hasSession: true, tokenLength: ctx.session.token.length }
  })
}
"#;
    let temp = /* tempdir */;
    let plugin_root = write_fixture(&temp, source);
    let mut harness = WorkerHarness::start(&node, &plugin_root);
    harness.send(&init_frame(&plugin_root));
    match harness.next_frame() {
        ChildFrame::Ready { .. } => {}
        other => panic!("unexpected frame: {other:?}"),
    }

    let token = "t".repeat(32);
    harness.send(&ParentFrame::InvokeCommand {
        call_id: 7,
        command_id: "probe".to_owned(),
        args: None,
        session: Some(SessionRef { token: token.clone() }),
    });

    match harness.next_frame() {
        ChildFrame::HostCall { call_id, method, params } => {
            assert_eq!(method, "session.post");
            assert_eq!(
                params,
                Some(json!({ "sessionToken": token, "topic": "log", "payload": { "line": "hi" } }))
            );
            harness.send(&ParentFrame::HostResult {
                call_id,
                ok: true,
                value: Some(json!({ "ok": true })),
                error_code: None,
                error: None,
            });
        }
        other => panic!("unexpected frame: {other:?}"),
    }
    match harness.next_frame() {
        ChildFrame::CommandResult { ok: true, value, .. } => {
            assert_eq!(value, Some(json!({ "hasSession": true, "tokenLength": 32 })));
        }
        other => panic!("unexpected frame: {other:?}"),
    }
}
```

（`SessionRef` 需在测试文件 use 列表补充。另加一条：不带 session 的 InvokeCommand → `{ "hasSession": false }`。）

- [ ] **Step 7: 运行**

Run: `cargo test -p ade-plugins --test host_entry` 与 `cargo test -p ade-plugins --lib` 与 `cargo test -p ade-bridge`
Expected: PASS

- [ ] **Step 8: 提交**

```bash
git add src-tauri/crates/ade-plugins src-tauri/crates/ade-bridge src-tauri/resources/plugins/host-entry.mjs
git commit -m "feat(plugins): worker.invokeCommand 全链与会话上下文透传（T5）"
```

## Task 6: `visibility` 贯通 bridge DTO 与 TS 类型（bindings 重生成）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/plugins.rs`（`PluginCommandDto` + 映射）
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`
- Modify: `src/shared/preload-api/api/plugin-host-api.ts`（命令条目类型 + `visibility`）

**Interfaces:**
- Consumes: T2 的 `CommandVisibility`。
- Produces: `PluginCommandDto.visibility: "palette" | "internal"`（bindings 与 TS 类型同形）。供 T7/T9 使用。

- [ ] **Step 1: Rust DTO 修改（commands/plugins.rs）**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PluginCommandVisibilityDto {
    Palette,
    Internal,
}

impl From<ade_plugins::manifest::CommandVisibility> for PluginCommandVisibilityDto {
    fn from(value: ade_plugins::manifest::CommandVisibility) -> Self {
        match value {
            ade_plugins::manifest::CommandVisibility::Palette => Self::Palette,
            ade_plugins::manifest::CommandVisibility::Internal => Self::Internal,
        }
    }
}
```

`PluginCommandDto`（L137-145）增字段：

```rust
pub visibility: PluginCommandVisibilityDto,
```

命令映射（~L264 `PluginCommandDto {` 构造处）增 `visibility: command.visibility.into(),`（变量名按现场）。

- [ ] **Step 2: 重生成 bindings 并跑新鲜度测试**

Run: `cargo run -p ade-bridge --bin export-bindings`（`src-tauri/`）→ 仓库根 `git diff --exit-code -- src/bridge/real/generated/tauri-bindings.ts` 应显示 diff；
Run: `cargo test -p ade-bridge bindings_are_fresh`（`src-tauri/`）
Expected: 重生成后 PASS

- [ ] **Step 3: TS 契约类型（src/shared/preload-api/api/plugin-host-api.ts）**

`PluginHostListEntry.commands` 条目类型增：

```ts
  commands: {
    id: string
    title: string
    context: 'global' | 'worktree'
    handler: { type: 'built-in'; action: string } | { type: 'worker' }
    keybindings: { key: string; when: 'global' | 'worktree' }[]
    /** `internal` commands are UI-only: never surfaced in the palette or shortcut surfaces. */
    visibility: 'palette' | 'internal'
  }[]
```

- [ ] **Step 4: 修 fixture/测试的编译破坏**

Run: `pnpm typecheck`
Expected: 报出所有缺 `visibility` 的命令 fixture 构造点；逐一补 `visibility: 'palette'`（或按语义 `'internal'`）。典型位置：`src/bridge/mock/fixtures.ts`（当前 commands 为 `[]`，可能无需改）、快捷键面板/命令相关测试的 fixture。
Run: `pnpm test`（仅受影响文件的定向运行：`pnpm vitest run src/bridge src/renderer/src/store/plugin-panels.test.ts`）
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/ade-bridge/src/commands/plugins.rs src/bridge/real/generated/tauri-bindings.ts src/shared/preload-api/api/plugin-host-api.ts
git commit -m "feat(plugins): visibility 贯通 bridge DTO/bindings/TS 类型（T6）"
```

## Task 7: TS 通道契约（能力联合、spec 表、push 消息、订阅 25→26）

**Files:**
- Modify: `src/shared/plugins/plugin-capabilities.ts`（kinds + descriptions）
- Modify: `src/shared/plugins/plugin-host-api.ts`（`PLUGIN_HOST_API_V0` +2 条）
- Modify: `src/shared/plugins/plugin-panel-bridge.ts`（`orca-panel-push` 常量/schema/构造/解析）
- Modify: `src/shared/preload-api/api/plugin-host-api.ts`（`PluginPanelPushRequest` + `onPanelPush`）
- Modify: `src/bridge/mock/plugins-api.ts`（no-op 订阅）
- Modify: `src/bridge/real/plugins.ts`（`onPanelPush` 订阅）
- Modify: `src/bridge/real/parity.test.ts`（disposition + 26）
- Modify: `src/bridge/real/plugins.test.ts`（订阅事件测试）
- Test: `src/shared/plugins/plugin-panel-bridge.test.ts`（若无则新建）、上述各文件既有测试

**Interfaces:**
- Consumes: 事件 `plugins:panel-push`（T4）。
- Produces: `PANEL_PUSH_TYPE = 'orca-panel-push'`、`createPanelPushMessage`、`parsePanelPushMessage`、`PluginPanelPushRequest`、`PluginsApi.onPanelPush`。供 T8 使用。

- [ ] **Step 1: 写失败测试**

`src/shared/plugins/plugin-panel-bridge.test.ts`（新建或追加）：

```ts
import { describe, expect, it } from 'vitest'
import {
  PANEL_PUSH_TYPE,
  createPanelPushMessage,
  parsePanelPushMessage
} from './plugin-panel-bridge'

describe('orca-panel-push', () => {
  it('round-trips topic and opaque payload', () => {
    const message = createPanelPushMessage('log', { line: 'hi' })
    expect(message).toEqual({ type: PANEL_PUSH_TYPE, topic: 'log', payload: { line: 'hi' } })
    expect(parsePanelPushMessage(message)).toEqual({ ok: true, topic: 'log', payload: { line: 'hi' } })
  })

  it('rejects malformed pushes and oversize topics', () => {
    expect(parsePanelPushMessage({ type: 'other' }).ok).toBe(false)
    expect(parsePanelPushMessage({ type: PANEL_PUSH_TYPE, topic: 'x'.repeat(65), payload: 1 }).ok).toBe(false)
    expect(parsePanelPushMessage({ type: PANEL_PUSH_TYPE }).ok).toBe(false)
  })

  it('omits an absent topic', () => {
    expect(createPanelPushMessage(undefined, 1)).toEqual({ type: PANEL_PUSH_TYPE, payload: 1 })
  })
})
```

`src/bridge/real/plugins.test.ts`（events describe 内追加，仿既有 onOpenTab 断言）：

```ts
  it('subscribes onPanelPush to plugins:panel-push', async () => {
    const { api, listen } = /* 既有夹具 */;
    const callback = vi.fn()
    const unsubscribe = api.onPanelPush(callback)
    const handler = listen.mock.calls.find(([event]) => event === 'plugins:panel-push')?.[1]
    handler?.({ payload: { sessionToken: 't', payload: 1 } })
    expect(callback).toHaveBeenCalledWith({ sessionToken: 't', payload: 1 })
    unsubscribe()
  })
```

（对齐该文件既有 `onOpenTab` 测试的写法与夹具变量名。）

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/shared/plugins/plugin-panel-bridge.test.ts src/bridge/real/plugins.test.ts`
Expected: FAIL（导出/成员不存在）

- [ ] **Step 3: 实现（shared）**

`plugin-capabilities.ts`：

```ts
export const PLUGIN_CAPABILITY_KINDS = [
  'events:subscribe',
  'notifications:show',
  'secrets',
  'session:push',
  'settings:own',
  'storage',
  'terminal:send',
  'worker:call',
  'workspace:open-tab',
  'workspace:read'
] as const
```

`PLUGIN_CAPABILITY_DESCRIPTIONS` 增两条：

```ts
  'worker:call':
    "Allow this plugin's panels and tabs to invoke its own background commands (worker RPC).",
  'session:push':
    "Allow this plugin's background worker to push live updates into its open panels and tabs.",
```

`plugin-host-api.ts`（`PLUGIN_HOST_API_V0` 数组；插入位置与 Rust 表一致）：

```ts
  {
    name: 'worker.invokeCommand',
    since: '1.1',
    scope: 'plugin-private',
    stability: 'experimental',
    capability: 'worker:call',
    mutation: false,
    panel: true,
    params: z
      .object({ commandId: z.string().min(1).max(256), args: z.json().optional() })
      .strict(),
    result: z.json()
  },
```

```ts
  {
    name: 'session.post',
    since: '1.1',
    scope: 'plugin-private',
    stability: 'experimental',
    capability: 'session:push',
    mutation: false,
    panel: false,
    params: z
      .object({
        sessionToken: z.string().min(32).max(128),
        topic: z.string().min(1).max(64).optional(),
        payload: z.json()
      })
      .strict(),
    result: z.object({ ok: z.literal(true) }).strict()
  }
```

`plugin-panel-bridge.ts`：

```ts
/** Host→guest push carrying worker-streamed updates (spec §3.3). */
export const PANEL_PUSH_TYPE = 'orca-panel-push'

export const panelPushMessageSchema = z.object({
  type: z.literal(PANEL_PUSH_TYPE),
  topic: z.string().min(1).max(64).optional(),
  payload: z.unknown()
})

export type PluginPanelPushMessage = z.infer<typeof panelPushMessageSchema>

export function createPanelPushMessage(topic: string | undefined, payload: unknown): PluginPanelPushMessage {
  return topic === undefined ? { type: PANEL_PUSH_TYPE, payload } : { type: PANEL_PUSH_TYPE, topic, payload }
}

export type PanelPushMessageParseResult =
  | { ok: true; topic: string | undefined; payload: unknown }
  | { ok: false }

export function parsePanelPushMessage(data: unknown): PanelPushMessageParseResult {
  const parsed = panelPushMessageSchema.safeParse(data)
  return parsed.success
    ? { ok: true, topic: parsed.data.topic, payload: parsed.data.payload }
    : { ok: false }
}
```

- [ ] **Step 4: 实现（preload/mock/real/parity）**

`src/shared/preload-api/api/plugin-host-api.ts`：

```ts
/** Payload of the `plugins:panel-push` bridge event: worker-streamed data for
 *  one open UI session (panel or tab), matched by `sessionToken`. */
export type PluginPanelPushRequest = {
  sessionToken: string
  topic?: string
  payload: unknown
}
```

`PluginsApi` 增：

```ts
  /** Subscribes to worker pushes targeting this renderer's plugin sessions. */
  onPanelPush: (callback: (push: PluginPanelPushRequest) => void) => () => void
```

`src/bridge/mock/plugins-api.ts`：

```ts
    onPanelPush: () => () => {},
```

`src/bridge/real/plugins.ts`：

```ts
    onPanelPush: (callback) =>
      subscribeToEvent<PluginPanelPushRequest>('plugins:panel-push', callback),
```

`parity.test.ts`：disposition 表增 `onPanelPush: 'explicit',`；`toHaveLength(25)` 改 `26`，注释改 "23 commands + 3 subscriptions"；surfaceCase 注释同步。

- [ ] **Step 5: 运行**

Run: `pnpm vitest run src/shared/plugins src/bridge`
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add src/shared/plugins src/shared/preload-api src/bridge
git commit -m "feat(plugins): TS 侧通道契约（spec 表/推送消息/onPanelPush 26 成员）（T7）"
```

## Task 8: 渲染层推送转发（token→iframe）

**Files:**
- Create: `src/renderer/src/components/right-sidebar/plugin-panel-push.ts`
- Create: `src/renderer/src/components/right-sidebar/plugin-panel-push.test.ts`
- Modify: `src/renderer/src/components/right-sidebar/use-plugin-panel-entry.ts`

**Interfaces:**
- Consumes: T7 的 `PluginPanelPushRequest`、`createPanelPushMessage`。
- Produces: `forwardPanelPush(getWindow, sessionToken, push): boolean`。三个宿主组件经共用 hook 自动获得转发，无组件级改动。

- [ ] **Step 1: 写失败测试**

```ts
import { describe, expect, it, vi } from 'vitest'
import { forwardPanelPush } from './plugin-panel-push'

function fakeWindow() {
  return { postMessage: vi.fn() } as unknown as Window
}

describe('forwardPanelPush', () => {
  it('posts a camelCase push into the matching session frame', () => {
    const target = fakeWindow()
    const forwarded = forwardPanelPush(() => target, 'token-a', {
      sessionToken: 'token-a',
      topic: 'state',
      payload: { n: 1 }
    })
    expect(forwarded).toBe(true)
    expect(target.postMessage).toHaveBeenCalledWith(
      { type: 'orca-panel-push', topic: 'state', payload: { n: 1 } },
      '*'
    )
  })

  it('drops pushes for other sessions and missing frames', () => {
    const target = fakeWindow()
    expect(
      forwardPanelPush(() => target, 'token-a', { sessionToken: 'token-b', payload: 1 })
    ).toBe(false)
    expect(
      forwardPanelPush(() => null, 'token-a', { sessionToken: 'token-a', payload: 1 })
    ).toBe(false)
    expect(
      forwardPanelPush(() => target, null, { sessionToken: 'token-a', payload: 1 })
    ).toBe(false)
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/renderer/src/components/right-sidebar/plugin-panel-push.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现**

```ts
import type { PluginPanelPushRequest } from '../../../../shared/preload-api/api/plugin-host-api'
import { createPanelPushMessage } from '../../../../shared/plugins/plugin-panel-bridge'

/** Forwards one `plugins:panel-push` payload into the plugin iframe whose
 *  session matches. Returns false when the push is not for this frame. */
export function forwardPanelPush(
  getPanelWindow: () => Window | null,
  sessionToken: string | null,
  push: PluginPanelPushRequest
): boolean {
  if (!sessionToken || push.sessionToken !== sessionToken) {
    return false
  }
  const target = getPanelWindow()
  if (!target) {
    return false
  }
  target.postMessage(createPanelPushMessage(push.topic, push.payload), '*')
  return true
}
```

- [ ] **Step 4: 接入共用 hook（use-plugin-panel-entry.ts）**

在 `sessionToken` 与 `iframeRef` 均已存在的组件作用域内新增 effect（放在既有 watchdog/message-handler effect 邻近；`sessionToken` 已在 hook state 中）：

```ts
  useEffect(() => {
    const plugins = window.api?.plugins
    if (!plugins?.onPanelPush || !sessionToken) {
      return
    }
    return plugins.onPanelPush((push) => {
      forwardPanelPush(() => iframeRef.current?.contentWindow ?? null, sessionToken, push)
    })
  }, [sessionToken])
```

（导入 `forwardPanelPush`；`iframeRef` 为 hook 既有 ref。）

- [ ] **Step 5: 运行**

Run: `pnpm vitest run src/renderer/src/components/right-sidebar/plugin-panel-push.test.ts src/renderer/src/components/right-sidebar/plugin-panel-watchdog.test.ts`
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add src/renderer/src/components/right-sidebar/plugin-panel-push.ts src/renderer/src/components/right-sidebar/plugin-panel-push.test.ts src/renderer/src/components/right-sidebar/use-plugin-panel-entry.ts
git commit -m "feat(plugins): 渲染层按会话 token 转发 panel-push（T8）"
```

## Task 9: internal 命令过滤（命令面板/快捷键消费面）

**Files:**
- Modify: `src/renderer/src/store/plugin-panels.ts`（`collectActivePluginCommands` / `collectEditablePluginCommands`）
- Test: `src/renderer/src/store/plugin-panels.test.ts`、`src/renderer/src/components/cmd-j/plugin-quick-actions.test.ts`（若无该文件则在 `store` 测试内断言）

**Interfaces:**
- Consumes: T6 的 `visibility` 字段。
- Produces: 收集器不再输出 `visibility === 'internal'` 的命令（palette、`findPluginCommandForKeybinding`、ShortcutsPane 三个消费面同时生效）。

- [ ] **Step 1: 写失败测试（plugin-panels.test.ts 追加）**

```ts
  it('hides internal commands from palette and shortcut collectors', async () => {
    // 复用本文件既有的 mock window.api.plugins 夹具方式（见文件顶部既有测试），
    // 构造一个含两条命令的列表条目：
    //   { id: 'check', visibility: 'palette', ... }
    //   { id: 'snapshot', visibility: 'internal', ... }
    await ensurePluginPanelsLoaded()
    const visible = collectActivePluginCommands()
    expect(visible.map((command) => command.id)).toEqual(['check'])
    const editable = collectEditablePluginCommands()
    expect(editable.map((command) => command.id)).toEqual(['check'])
  })
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/renderer/src/store/plugin-panels.test.ts`
Expected: FAIL（internal 命令仍出现）

- [ ] **Step 3: 实现（store/plugin-panels.ts）**

`collectActivePluginCommands` 与 `collectEditablePluginCommands` 的 entries 遍历中，对 `entry.commands` 增加过滤：

```ts
      .flatMap((entry) =>
        entry.commands
          .filter((command) => command.visibility !== 'internal')
          .map((command) => ({ ...command, pluginKey: entry.pluginKey, pluginName: entry.name, scope: entry.scope, repoId: entry.repoId }))
      )
```

（两处按现场结构修改；保持既有类型与字段一致。）

- [ ] **Step 4: 运行**

Run: `pnpm vitest run src/renderer/src/store/plugin-panels.test.ts src/renderer/src/components/cmd-j`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/store/plugin-panels.ts src/renderer/src/store/plugin-panels.test.ts
git commit -m "feat(plugins): 命令面板与快捷键面过滤 internal 命令（T9）"
```

---

# B 部分：updater 插件（`plugins/updater/`）

## Task 10: 插件脚手架与构建管线

**Files:**
- Create: `plugins/updater/ade-plugin.json`
- Create: `plugins/updater/build.mjs`
- Create: `plugins/updater/tsconfig.worker.json`
- Create: `plugins/updater/tsconfig.ui.json`
- Create: `plugins/updater/src/worker/main.ts`（占位，T15 替换）
- Create: `plugins/updater/src/ui/theme.css`（占位，T16 替换）
- Create: `plugins/updater/src/ui/tab.html`、`plugins/updater/src/ui/panel.html`
- Create: `plugins/updater/src/ui/tab/main.tsx`、`plugins/updater/src/ui/panel/main.tsx`（占位，T17 替换）
- Modify: `package.json`（scripts）
- Modify: `vitest.config.ts`（include）
- Modify: `.gitignore`

**Interfaces:**
- Produces: `pnpm build:updater` → `plugins/updater/dist/{main.mjs,tab.html,panel.html}`（单文件）；后续所有插件任务的构建入口。

- [ ] **Step 1: 写 manifest（与 spec §3.5 逐字一致；`icon` 用 curated 映射内已有的 `package`，见 `plugin-panel-activity-items.ts`）**

```json
{
  "manifestVersion": 1,
  "id": "updater",
  "publisher": "itsuka",
  "name": "Package Updater",
  "version": "0.1.0",
  "description": "Check and update global npm packages and Homebrew formulae/casks.",
  "engines": { "orca": ">=0.0.1" },
  "pluginApi": 1,
  "main": "dist/main.mjs",
  "contributes": {
    "tabs": [{ "id": "updates", "title": "Updates", "icon": "package", "entry": "dist/tab.html" }],
    "panels": [{ "id": "updater", "title": "Package Updater", "icon": "package", "entry": "dist/panel.html" }],
    "commands": [
      { "id": "check", "title": "Updates: Check for updates" },
      { "id": "update-all", "title": "Updates: Update all packages" },
      { "id": "cancel", "title": "Updates: Cancel running update" },
      { "id": "open", "title": "Updates: Open updater tab" },
      { "id": "snapshot", "title": "Updates: Get state", "visibility": "internal" },
      { "id": "set-managers", "title": "Updates: Set enabled managers", "visibility": "internal" }
    ]
  },
  "capabilities": [
    { "kind": "worker:call" },
    { "kind": "session:push" },
    { "kind": "workspace:open-tab" },
    { "kind": "storage" }
  ]
}
```

- [ ] **Step 2: 构建脚本（完整写盘）**

`plugins/updater/build.mjs`：

```js
#!/usr/bin/env node
import { rm } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { build as esbuildBuild } from 'esbuild'
import { build as viteBuild } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const root = path.dirname(fileURLToPath(import.meta.url))
const distDir = path.join(root, 'dist')
const uiRoot = path.join(root, 'src', 'ui')

// Inlines every emitted JS/CSS asset into its HTML entry: the panel CSP allows
// only inline scripts/styles, so dist entries must be single-file documents.
function singleFileInline() {
  return {
    name: 'updater-single-file-inline',
    enforce: 'post',
    generateBundle(_options, bundle) {
      for (const html of Object.values(bundle)) {
        if (html.type !== 'asset' || !html.fileName.endsWith('.html')) continue
        let source = String(html.source)
        for (const [fileName, file] of Object.entries(bundle)) {
          if (file === html) continue
          const escaped = fileName.replaceAll('.', '\\.')
          if (file.type === 'chunk') {
            const pattern = new RegExp(`<script[^>]*src="[^"]*${escaped}"[^>]*></script>`)
            const code = file.code.replaceAll('</script', '<\\/script')
            source = source.replace(pattern, `<script type="module">${code}</script>`)
            delete bundle[fileName]
          } else if (fileName.endsWith('.css')) {
            const pattern = new RegExp(`<link[^>]*href="[^"]*${escaped}"[^>]*>`)
            source = source.replace(pattern, `<style>${String(file.source)}</style>`)
            delete bundle[fileName]
          }
        }
        html.source = source
      }
    }
  }
}

await rm(distDir, { recursive: true, force: true })

await esbuildBuild({
  entryPoints: [path.join(root, 'src', 'worker', 'main.ts')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node18',
  outfile: path.join(distDir, 'main.mjs'),
  logLevel: 'warning'
})

await viteBuild({
  configFile: false,
  root: uiRoot,
  publicDir: false,
  base: './',
  logLevel: 'warn',
  plugins: [react(), tailwindcss(), singleFileInline()],
  build: {
    target: 'es2022',
    outDir: distDir,
    emptyOutDir: false,
    assetsInlineLimit: 128 * 1024,
    modulePreload: false,
    cssCodeSplit: false,
    sourcemap: false,
    rollupOptions: {
      input: {
        tab: path.join(uiRoot, 'tab.html'),
        panel: path.join(uiRoot, 'panel.html')
      }
    }
  }
})

console.log(`updater plugin built -> ${distDir}`)
```

- [ ] **Step 3: tsconfig 分层**

`plugins/updater/tsconfig.worker.json`：

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022"],
    "types": ["node"],
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "esModuleInterop": true
  },
  "include": ["src/worker/**/*.ts", "src/shared/**/*.ts"]
}
```

`plugins/updater/tsconfig.ui.json`：

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "jsx": "react-jsx",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": []
  },
  "include": ["src/ui/**/*.ts", "src/ui/**/*.tsx", "src/shared/**/*.ts"]
}
```

- [ ] **Step 4: 入口与 HTML（含占位实现）**

`src/ui/tab.html`：

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Updates</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="./tab/main.tsx"></script>
  </body>
</html>
```

`src/ui/panel.html`：同构，`src` 改 `./panel/main.tsx`，`<title>Package Updater</title>`。

`src/ui/tab/main.tsx`：

```tsx
import { createRoot } from 'react-dom/client'
import '../theme.css'

createRoot(document.getElementById('root') as HTMLElement).render(
  <main style={{ padding: 12 }}>Updates</main>
)
```

`src/ui/panel/main.tsx`：同构，文案 `Package Updater`。

`src/ui/theme.css`：

```css
@import 'tailwindcss';
```

`src/worker/main.ts`：

```ts
export default function activate(_orca: unknown): void {
  // Command wiring lands in Task 15.
}
```

- [ ] **Step 5: 根仓库接线**

`package.json` scripts 修改为：

```json
    "build:updater": "node plugins/updater/build.mjs",
    "typecheck": "tsc --noEmit -p tsconfig.json && tsc --noEmit -p plugins/updater/tsconfig.worker.json && tsc --noEmit -p plugins/updater/tsconfig.ui.json",
```

`vitest.config.ts` 的 `include` 增一行：

```ts
    include: [
      'src/**/*.test.ts',
      'src/**/*.test.tsx',
      'plugins/**/*.test.ts',
      'config/scripts/**/*.test.mjs'
    ],
```

`.gitignore` 末尾追加：

```
plugins/updater/dist/
```

- [ ] **Step 6: 构建并断言产物**

Run: `pnpm build:updater`
Expected: exit 0；`dist/main.mjs`、`dist/tab.html`、`dist/panel.html` 存在。

Run（单文件与语法断言）：

```bash
node --check plugins/updater/dist/main.mjs
node -e "const fs=require('fs');for(const f of ['plugins/updater/dist/tab.html','plugins/updater/dist/panel.html']){const s=fs.readFileSync(f,'utf8');if(/<script[^>]*\ssrc=/.test(s))throw new Error(f+': external script');if(/<link[^>]*rel=\"stylesheet\"/.test(s))throw new Error(f+': external css');if(!s.includes('<style>'))throw new Error(f+': missing inline css')}console.log('single-file ok')"
```

Expected: `single-file ok`

- [ ] **Step 7: typecheck**

Run: `pnpm typecheck`
Expected: 三个 tsconfig 全部 exit 0

- [ ] **Step 8: 提交**

```bash
git add plugins/updater package.json vitest.config.ts .gitignore
git commit -m "feat(updater): 插件脚手架与单文件构建管线（T10）"
```

## Task 11: worker 解析层（`parse.ts`）

**Files:**
- Create: `plugins/updater/src/shared/types.ts`
- Create: `plugins/updater/src/worker/parse.ts`
- Test: `plugins/updater/src/worker/parse.test.ts`

**Interfaces:**
- Produces: `ManagerId`、`PackageKind`、`OutdatedEntry`（shared）；`parseNpmOutdated(stdout): OutdatedEntry[]`、`parseBrewOutdated(stdout): OutdatedEntry[]`。供 T13/T14/T17 使用。

- [ ] **Step 1: 写失败测试**

`src/shared/types.ts`（本任务先落域类型）：

```ts
/** Shared domain types between the updater worker and its UI bundles. */

export type ManagerId = 'npm' | 'brew'

export type PackageKind = 'global' | 'formula' | 'cask'

export type OutdatedEntry = {
  manager: ManagerId
  kind: PackageKind
  name: string
  current: string
  latest: string
}
```

`src/worker/parse.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import { parseBrewOutdated, parseNpmOutdated } from './parse'

const NPM_OUTDATED = JSON.stringify({
  typescript: { current: '5.8.3', wanted: '5.8.3', latest: '5.9.2', location: 'global' },
  eslint: { current: '9.18.0', wanted: '9.21.0', latest: '9.21.0', location: 'global' }
})

const BREW_OUTDATED = JSON.stringify({
  formulae: [
    { name: 'node', installed_versions: ['22.13.0'], current_version: '22.21.0' },
    { name: 'git', installed_versions: ['2.47.1'], current_version: '2.49.0' }
  ],
  casks: [{ name: 'docker-desktop', installed_versions: ['4.38.0'], current_version: '4.40.0' }]
})

describe('parseNpmOutdated', () => {
  it('maps the npm json into sorted entries', () => {
    expect(parseNpmOutdated(NPM_OUTDATED)).toEqual([
      { manager: 'npm', kind: 'global', name: 'eslint', current: '9.18.0', latest: '9.21.0' },
      { manager: 'npm', kind: 'global', name: 'typescript', current: '5.8.3', latest: '5.9.2' }
    ])
  })

  it('treats empty stdout and {} as no updates', () => {
    expect(parseNpmOutdated('')).toEqual([])
    expect(parseNpmOutdated('{}')).toEqual([])
  })

  it('throws on malformed json (caller surfaces the raw failure)', () => {
    expect(() => parseNpmOutdated('npm ERR! network')).toThrowError()
  })
})

describe('parseBrewOutdated', () => {
  it('maps formulae and casks', () => {
    expect(parseBrewOutdated(BREW_OUTDATED)).toEqual([
      { manager: 'brew', kind: 'cask', name: 'docker-desktop', current: '4.38.0', latest: '4.40.0' },
      { manager: 'brew', kind: 'formula', name: 'git', current: '2.47.1', latest: '2.49.0' },
      { manager: 'brew', kind: 'formula', name: 'node', current: '22.13.0', latest: '22.21.0' }
    ])
  })

  it('handles missing arrays, empty stdout, and skips nameless items', () => {
    expect(parseBrewOutdated('')).toEqual([])
    expect(parseBrewOutdated('{}')).toEqual([])
    expect(parseBrewOutdated(JSON.stringify({ formulae: [{ current_version: '1.0.0' }] }))).toEqual([])
  })

  it('throws on malformed json', () => {
    expect(() => parseBrewOutdated('Warning: something')).toThrowError()
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run plugins/updater/src/worker/parse.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现（`src/worker/parse.ts`）**

```ts
import type { OutdatedEntry, PackageKind } from '../shared/types'

type NpmOutdatedJson = Record<string, { current?: unknown; latest?: unknown }>

/** `npm outdated -g --json`: exit 1 with output when updates exist, `{}` when none. */
export function parseNpmOutdated(stdout: string): OutdatedEntry[] {
  const trimmed = stdout.trim()
  if (trimmed === '' || trimmed === '{}') {
    return []
  }
  const data = JSON.parse(trimmed) as NpmOutdatedJson
  const entries: OutdatedEntry[] = []
  for (const [name, value] of Object.entries(data)) {
    entries.push({
      manager: 'npm',
      kind: 'global',
      name,
      current: typeof value?.current === 'string' ? value.current : '',
      latest: typeof value?.latest === 'string' ? value.latest : ''
    })
  }
  return entries.sort((a, b) => a.name.localeCompare(b.name))
}

type BrewOutdatedV2Item = {
  name?: unknown
  installed_versions?: unknown
  current_version?: unknown
}

type BrewOutdatedV2 = {
  formulae?: BrewOutdatedV2Item[]
  casks?: BrewOutdatedV2Item[]
}

/** `brew outdated --json=v2`: exit 0 with `{ formulae, casks }`. */
export function parseBrewOutdated(stdout: string): OutdatedEntry[] {
  const trimmed = stdout.trim()
  if (trimmed === '') {
    return []
  }
  const data = JSON.parse(trimmed) as BrewOutdatedV2
  const entries: OutdatedEntry[] = []
  const collect = (kind: PackageKind, items: BrewOutdatedV2Item[] | undefined) => {
    for (const item of items ?? []) {
      if (typeof item?.name !== 'string' || item.name.length === 0) {
        continue
      }
      const current =
        Array.isArray(item.installed_versions) && typeof item.installed_versions[0] === 'string'
          ? (item.installed_versions[0] as string)
          : ''
      entries.push({
        manager: 'brew',
        kind,
        name: item.name,
        current,
        latest: typeof item.current_version === 'string' ? item.current_version : ''
      })
    }
  }
  collect('formula', data.formulae)
  collect('cask', data.casks)
  return entries.sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name))
}
```

- [ ] **Step 4: 运行通过**

Run: `pnpm vitest run plugins/updater/src/worker/parse.test.ts`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add plugins/updater/src/shared/types.ts plugins/updater/src/worker/parse.ts plugins/updater/src/worker/parse.test.ts
git commit -m "feat(updater): npm/brew outdated 解析层（T11）"
```

## Task 12: worker 进程执行层（`runner.ts`）

**Files:**
- Create: `plugins/updater/src/worker/runner.ts`
- Test: `plugins/updater/src/worker/runner.test.ts`

**Interfaces:**
- Produces: `RunResult`、`RunOptions`、`runCommand(cmd, args, options?): Promise<RunResult>`、`MAX_LINE_LENGTH = 4096`、`DEFAULT_TIMEOUT_MS = 120_000`。供 T13 使用。

- [ ] **Step 1: 写失败测试**

`src/worker/runner.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import { MAX_LINE_LENGTH, runCommand } from './runner'

describe('runCommand', () => {
  it('captures lines, streams them, and reports the exit code', async () => {
    const lines: Array<{ line: string; stream: string }> = []
    const result = await runCommand(
      process.execPath,
      ['-e', "console.log('one'); console.error('two'); process.exit(3)"],
      { onLine: (line, stream) => lines.push({ line, stream }) }
    )
    expect(result.exitCode).toBe(3)
    expect(result.stdout).toContain('one')
    expect(result.stderr).toContain('two')
    expect(lines).toEqual(
      expect.arrayContaining([
        { line: 'one', stream: 'stdout' },
        { line: 'two', stream: 'stderr' }
      ])
    )
  })

  it('truncates over-long lines to MAX_LINE_LENGTH', async () => {
    const result = await runCommand(process.execPath, ['-e', "console.log('x'.repeat(9000))"])
    expect(result.stdout.trimEnd().length).toBe(MAX_LINE_LENGTH)
  })

  it('kills the child on abort with SIGTERM', async () => {
    const abort = new AbortController()
    const pending = runCommand(process.execPath, ['-e', 'setTimeout(() => {}, 60000)'], {
      signal: abort.signal,
      killGraceMs: 100
    })
    setTimeout(() => abort.abort(), 50)
    const result = await pending
    expect(result.signal).toBe('SIGTERM')
  })

  it('enforces the timeout', async () => {
    const result = await runCommand(process.execPath, ['-e', 'setTimeout(() => {}, 60000)'], {
      timeoutMs: 100,
      killGraceMs: 100
    })
    expect(result.signal).toBe('SIGTERM')
  })

  it('rejects when the binary cannot be spawned', async () => {
    await expect(runCommand('__ade_missing_binary__', [])).rejects.toThrowError()
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run plugins/updater/src/worker/runner.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现（`src/worker/runner.ts`）**

```ts
import { spawn } from 'node:child_process'

export type RunStream = 'stdout' | 'stderr'

export type RunResult = {
  exitCode: number | null
  signal: string | null
  stdout: string
  stderr: string
}

export type RunOptions = {
  onLine?: (line: string, stream: RunStream) => void
  signal?: AbortSignal
  timeoutMs?: number
  killGraceMs?: number
}

export const MAX_LINE_LENGTH = 4096
export const DEFAULT_TIMEOUT_MS = 120_000

/**
 * Runs one child process with line-buffered capture, a hard timeout, and
 * cancel semantics: SIGTERM first, SIGKILL after `killGraceMs`. The final
 * partial line (no trailing newline) is intentionally dropped.
 */
export function runCommand(cmd: string, args: string[], options: RunOptions = {}): Promise<RunResult> {
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS
  const killGraceMs = options.killGraceMs ?? 2_000
  return new Promise<RunResult>((resolve, reject) => {
    const child = spawn(cmd, args, { env: process.env, stdio: ['ignore', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    let settled = false
    let killTimer: ReturnType<typeof setTimeout> | null = null

    const timeoutTimer = setTimeout(() => requestKill(), timeoutMs)

    function cleanup() {
      clearTimeout(timeoutTimer)
      if (killTimer !== null) {
        clearTimeout(killTimer)
      }
      options.signal?.removeEventListener('abort', onAbort)
    }

    function finish(run: () => void) {
      if (settled) {
        return
      }
      settled = true
      cleanup()
      run()
    }

    function requestKill() {
      if (child.exitCode !== null || child.signalCode !== null) {
        return
      }
      child.kill('SIGTERM')
      if (killTimer === null) {
        killTimer = setTimeout(() => child.kill('SIGKILL'), killGraceMs)
      }
    }

    function onAbort() {
      requestKill()
    }

    if (options.signal) {
      if (options.signal.aborted) {
        requestKill()
      } else {
        options.signal.addEventListener('abort', onAbort)
      }
    }

    const lineReader = (stream: RunStream, sink: (line: string) => void) => {
      let buffer = ''
      return (chunk: Buffer) => {
        buffer += chunk.toString('utf8')
        let index = buffer.indexOf('\n')
        while (index !== -1) {
          const raw = buffer.slice(0, index).replace(/\r$/, '')
          buffer = buffer.slice(index + 1)
          const line = raw.length > MAX_LINE_LENGTH ? raw.slice(0, MAX_LINE_LENGTH) : raw
          options.onLine?.(line, stream)
          sink(line + '\n')
          index = buffer.indexOf('\n')
        }
      }
    }

    child.stdout.on('data', lineReader('stdout', (line) => {
      stdout += line
    }))
    child.stderr.on('data', lineReader('stderr', (line) => {
      stderr += line
    }))
    child.on('error', (error) => finish(() => reject(error)))
    child.on('close', (exitCode, signal) =>
      finish(() => resolve({ exitCode, signal, stdout, stderr }))
    )
  })
}
```

- [ ] **Step 4: 运行通过**

Run: `pnpm vitest run plugins/updater/src/worker/runner.test.ts`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add plugins/updater/src/worker/runner.ts plugins/updater/src/worker/runner.test.ts
git commit -m "feat(updater): 进程执行层（行流/超时/取消）（T12）"
```

## Task 13: worker 管理器适配器注册表（`managers.ts`）

**Files:**
- Create: `plugins/updater/src/worker/managers.ts`
- Test: `plugins/updater/src/worker/managers.test.ts`

**Interfaces:**
- Consumes: T11 `parse*`、T12 `runCommand`/`RunResult`。
- Produces: `RunFn`、`ManagerAdapter{id,label,kinds,detect,outdated,upgradeArgs,upgrade}`、`createNpmAdapter(run?)`、`createBrewAdapter(run?)`、`defaultAdapters()`。供 T14/T15 使用。

- [ ] **Step 1: 写失败测试**

`src/worker/managers.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import type { RunResult } from './runner'
import { createBrewAdapter, createNpmAdapter, type RunFn } from './managers'

function ok(stdout: string): RunResult {
  return { exitCode: 0, signal: null, stdout, stderr: '' }
}

function result(exitCode: number | null, stdout: string, stderr = ''): RunResult {
  return { exitCode, signal: null, stdout, stderr }
}

describe('npm adapter', () => {
  it('accepts exit 1 from npm outdated as "updates exist" and parses stdout', async () => {
    const run = vi.fn<RunFn>(async () =>
      result(1, JSON.stringify({ typescript: { current: '5.8.3', latest: '5.9.2' } }), '')
    )
    const adapter = createNpmAdapter(run)
    const outdated = await adapter.outdated()
    expect(outdated.entries).toEqual([
      { manager: 'npm', kind: 'global', name: 'typescript', current: '5.8.3', latest: '5.9.2' }
    ])
    expect(run).toHaveBeenCalledWith('npm', ['outdated', '-g', '--json'])
  })

  it('treats other non-zero exits as hard failures', async () => {
    const run = vi.fn<RunFn>(async () => result(2, '', 'npm ERR! network'))
    const adapter = createNpmAdapter(run)
    await expect(adapter.outdated()).rejects.toThrowError(/npm outdated failed \(exit 2\)/)
  })

  it('detects npm from --version and builds latest upgrades', async () => {
    const run = vi.fn<RunFn>(async () => ok('10.15.0\n'))
    const adapter = createNpmAdapter(run)
    await expect(adapter.detect()).resolves.toBe(true)
    expect(adapter.upgradeArgs({ manager: 'npm', kind: 'global', name: 'tsx', current: 'a', latest: 'b' })).toEqual([
      'install',
      '-g',
      'tsx@latest'
    ])
  })

  it('reports missing npm when spawn throws', async () => {
    const run: RunFn = async () => {
      throw new Error('ENOENT')
    }
    await expect(createNpmAdapter(run).detect()).resolves.toBe(false)
  })
})

describe('brew adapter', () => {
  it('parses formulae and casks and maps cask upgrades to --cask', async () => {
    const stdout = JSON.stringify({
      formulae: [{ name: 'node', installed_versions: ['22.13.0'], current_version: '22.21.0' }],
      casks: [{ name: 'docker-desktop', installed_versions: ['4.38.0'], current_version: '4.40.0' }]
    })
    const run = vi.fn<RunFn>(async () => ok(stdout))
    const adapter = createBrewAdapter(run)
    const outdated = await adapter.outdated()
    expect(outdated.entries).toHaveLength(2)
    expect(
      adapter.upgradeArgs({ manager: 'brew', kind: 'cask', name: 'docker-desktop', current: 'a', latest: 'b' })
    ).toEqual(['upgrade', '--cask', 'docker-desktop'])
    expect(
      adapter.upgradeArgs({ manager: 'brew', kind: 'formula', name: 'node', current: 'a', latest: 'b' })
    ).toEqual(['upgrade', 'node'])
  })

  it('fails hard on non-zero exits', async () => {
    const run = vi.fn<RunFn>(async () => result(1, '', 'Error: not a formula'))
    await expect(createBrewAdapter(run).outdated()).rejects.toThrowError(/brew outdated failed \(exit 1\)/)
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run plugins/updater/src/worker/managers.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现（`src/worker/managers.ts`）**

```ts
import type { ManagerId, OutdatedEntry, PackageKind } from '../shared/types'
import { parseBrewOutdated, parseNpmOutdated } from './parse'
import { runCommand, type RunResult } from './runner'

export type RunFn = (cmd: string, args: string[], signal?: AbortSignal) => Promise<RunResult>

export type OutdatedResult = { entries: OutdatedEntry[]; raw: RunResult }

/** One package manager behind the registry: detection, outdated parsing, and
 *  per-package upgrade commands. Adding a manager = adding one adapter. */
export type ManagerAdapter = {
  id: ManagerId
  label: string
  kinds: PackageKind[]
  detect: () => Promise<boolean>
  outdated: () => Promise<OutdatedResult>
  upgradeArgs: (entry: OutdatedEntry) => string[]
  upgrade: (entry: OutdatedEntry, signal?: AbortSignal) => Promise<RunResult>
}

const defaultRun: RunFn = (cmd, args, signal) => runCommand(cmd, args, { signal })

export function createNpmAdapter(run: RunFn = defaultRun): ManagerAdapter {
  return {
    id: 'npm',
    label: 'npm',
    kinds: ['global'],
    detect: async () => {
      try {
        const result = await run('npm', ['--version'])
        return result.exitCode === 0
      } catch {
        return false
      }
    },
    outdated: async () => {
      const raw = await run('npm', ['outdated', '-g', '--json'])
      // npm outdated exits 1 when outdated packages exist; anything else is a failure.
      if (raw.exitCode !== 0 && raw.exitCode !== 1) {
        throw new Error(`npm outdated failed (exit ${raw.exitCode}): ${raw.stderr.trim()}`)
      }
      return { entries: parseNpmOutdated(raw.stdout), raw }
    },
    upgradeArgs: (entry) => ['install', '-g', `${entry.name}@latest`],
    upgrade: (entry, signal) => run('npm', ['install', '-g', `${entry.name}@latest`], signal)
  }
}

export function createBrewAdapter(run: RunFn = defaultRun): ManagerAdapter {
  return {
    id: 'brew',
    label: 'Homebrew',
    kinds: ['formula', 'cask'],
    detect: async () => {
      try {
        const result = await run('brew', ['--version'])
        return result.exitCode === 0
      } catch {
        return false
      }
    },
    outdated: async () => {
      const raw = await run('brew', ['outdated', '--json=v2'])
      if (raw.exitCode !== 0) {
        throw new Error(`brew outdated failed (exit ${raw.exitCode}): ${raw.stderr.trim()}`)
      }
      return { entries: parseBrewOutdated(raw.stdout), raw }
    },
    upgradeArgs: (entry) =>
      entry.kind === 'cask' ? ['upgrade', '--cask', entry.name] : ['upgrade', entry.name],
    upgrade: (entry, signal) =>
      run(
        'brew',
        entry.kind === 'cask' ? ['upgrade', '--cask', entry.name] : ['upgrade', entry.name],
        signal
      )
  }
}

export function defaultAdapters(): ManagerAdapter[] {
  return [createNpmAdapter(), createBrewAdapter()]
}
```

- [ ] **Step 4: 运行通过**

Run: `pnpm vitest run plugins/updater/src/worker/managers.test.ts`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add plugins/updater/src/worker/managers.ts plugins/updater/src/worker/managers.test.ts
git commit -m "feat(updater): 管理器适配器注册表（npm/brew）（T13）"
```

## Task 14: worker 任务状态机（`jobs.ts`）

**Files:**
- Modify: `plugins/updater/src/shared/types.ts`（状态类型与常量）
- Create: `plugins/updater/src/worker/jobs.ts`
- Test: `plugins/updater/src/worker/jobs.test.ts`

**Interfaces:**
- Consumes: T13 `ManagerAdapter`。
- Produces: `Updater` 类（`rememberSession(token)`、`snapshot()`、`check()`、`updateAll()`、`cancel()`、`setManagers(ids)`）、`UpdaterDeps`、`SetManagersResult`。供 T15（命令接线）与 T17（UI 类型）使用。

注：spec §3.6/§3.10 关于 `set-managers` 未知 id 的 `invalid_params` 措辞已更正为**结构化失败** `{ ok:false, error:'unknown_manager', manager }`（实现侧不抛错，保持 UI 控制流）；spec 修正已单独提交。

- [ ] **Step 1: 写失败测试**

`src/shared/types.ts` 追加：

```ts
export type JobPhase = 'idle' | 'checking' | 'updating' | 'done' | 'failed'

export type RowState = 'pending' | 'running' | 'updated' | 'failed' | 'skipped'

export type UpdateRow = OutdatedEntry & { state: RowState; error?: string }

export type ManagerState = {
  id: ManagerId
  label: string
  detected: boolean
  enabled: boolean
  count: number | null
  error?: string
}

export type UpdateSummary = {
  updated: number
  failed: number
  skipped: number
  elapsedMs: number
}

export type UpdaterState = {
  phase: JobPhase
  checkedAt: number | null
  managers: ManagerState[]
  rows: UpdateRow[]
  summary?: UpdateSummary
  error?: string
}

export const MANAGERS_STORAGE_KEY = 'managers'
export const LOG_FLUSH_INTERVAL_MS = 500
export const MAX_LOG_LINE_CHARS = 4096
export const MAX_LOG_CHUNK_BYTES = 48 * 1024
```

`src/worker/jobs.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import type { ManagerAdapter } from './managers'
import type { OutdatedEntry, UpdaterState } from '../shared/types'
import { Updater, type UpdaterDeps } from './jobs'

function entry(name: string, overrides: Partial<OutdatedEntry> = {}): OutdatedEntry {
  return { manager: 'npm', kind: 'global', name, current: '1.0.0', latest: '2.0.0', ...overrides }
}

type FakeAdapterOptions = {
  detected?: boolean
  entries?: OutdatedEntry[]
  outdatedDelayMs?: number
  failUpgradeFor?: string
  failOutdated?: boolean
}

function fakeAdapter(id: 'npm' | 'brew', options: FakeAdapterOptions = {}): ManagerAdapter & { upgrades: string[] } {
  const upgrades: string[] = []
  const adapter = {
    id,
    label: id === 'npm' ? 'npm' : 'Homebrew',
    kinds: id === 'npm' ? (['global'] as const).slice() : (['formula', 'cask'] as const).slice(),
    upgrades,
    detect: async () => options.detected ?? true,
    outdated: async () => {
      if (options.outdatedDelayMs) {
        await new Promise((resolve) => setTimeout(resolve, options.outdatedDelayMs))
      }
      if (options.failOutdated) {
        throw new Error(`${id} outdated exploded`)
      }
      return {
        entries: options.entries ?? [],
        raw: { exitCode: 0, signal: null, stdout: '', stderr: '' }
      }
    },
    upgradeArgs: () => [],
    upgrade: async (item: OutdatedEntry) => {
      upgrades.push(item.name)
      if (options.failUpgradeFor === item.name) {
        return { exitCode: 1, signal: null, stdout: '', stderr: `${item.name} exploded` }
      }
      return { exitCode: 0, signal: null, stdout: '', stderr: '' }
    }
  }
  return adapter as unknown as ManagerAdapter & { upgrades: string[] }
}

function harness(adapters: ManagerAdapter[], options: { sessions?: string[] } = {}) {
  const storage = new Map<string, unknown>()
  const posts: Array<{ token: string; topic: string; payload: unknown }> = []
  const failingTokens = new Set<string>()
  let clock = 1_000
  const deps: UpdaterDeps = {
    adapters,
    storageGet: async (key) => storage.get(key) ?? null,
    storageSet: async (key, value) => {
      storage.set(key, value)
    },
    postTo: async (token, topic, payload) => {
      if (failingTokens.has(token)) {
        const error = new Error('session is not available') as Error & { code?: string }
        error.code = 'unavailable'
        throw error
      }
      posts.push({ token, topic, payload })
    },
    now: () => (clock += 10),
    stateCoalesceMs: 0,
    logFlushMs: 0
  }
  const updater = new Updater(deps)
  for (const token of options.sessions ?? []) {
    updater.rememberSession(token)
  }
  return { updater, posts, storage, failingTokens }
}

describe('Updater.check', () => {
  it('detects managers, collects rows, and broadcasts the report', async () => {
    const npm = fakeAdapter('npm', { entries: [entry('a'), entry('b')] })
    const brew = fakeAdapter('brew', { entries: [entry('n', { manager: 'brew', kind: 'formula' })] })
    const { updater, posts } = harness([npm, brew], { sessions: ['tok-1'] })

    await expect(updater.check()).resolves.toEqual({ started: true, running: false })
    // 任务异步执行：轮询直到 phase 离开 checking
    await vi.waitFor(() => {
      const last = [...posts].reverse().find((post) => post.topic === 'state')
      const state = last?.payload as UpdaterState
      expect(state?.phase).toBe('done')
    })
    const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
    expect(state.rows.map((row) => row.name).sort()).toEqual(['a', 'b', 'n'])
    expect(state.managers.map((manager) => manager.count)).toEqual([2, 1])
  })

  it('is single-flight: a second check while running reports running', async () => {
    const npm = fakeAdapter('npm', { entries: [], outdatedDelayMs: 50 })
    const { updater } = harness([npm])
    await updater.check()
    await expect(updater.check()).resolves.toEqual({ started: false, running: true })
  })

  it('surfaces per-manager failures without failing the whole report', async () => {
    const npm = fakeAdapter('npm', { failOutdated: true })
    const brew = fakeAdapter('brew', { entries: [entry('n', { manager: 'brew', kind: 'formula' })] })
    const { updater, posts } = harness([npm, brew])
    await updater.check()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
      expect(state.managers.find((manager) => manager.id === 'npm')?.error).toContain('npm outdated exploded')
      expect(state.managers.find((manager) => manager.id === 'brew')?.count).toBe(1)
    })
  })
})

describe('Updater.updateAll', () => {
  it('upgrades sequentially, isolates failures, and flushes logs', async () => {
    const npm = fakeAdapter('npm', {
      entries: [entry('a'), entry('b'), entry('c')],
      failUpgradeFor: 'b'
    })
    const { updater, posts } = harness([npm], { sessions: ['tok-1'] })
    await updater.check()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
    })
    await expect(updater.updateAll()).resolves.toEqual({ started: true, running: false })
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
      expect(state?.summary).toEqual({ updated: 2, failed: 1, skipped: 0, elapsedMs: expect.any(Number) })
    })
    expect(npm.upgrades).toEqual(['a', 'b', 'c'])
    const logLines = posts.filter((post) => post.topic === 'log').flatMap((post) => (post.payload as { lines: string[] }).lines)
    expect(logLines.some((line) => line.includes('error: b'))).toBe(true)
  })

  it('cancels the running update and skips the rest', async () => {
    const npm = fakeAdapter('npm', { entries: [entry('a'), entry('b'), entry('c')] })
    // 让 a 的升级挂起直到 abort：
    const originalUpgrade = npm.upgrade
    npm.upgrade = (item, signal) =>
      item.name === 'a'
        ? new Promise((resolve) => {
            signal?.addEventListener('abort', () =>
              resolve({ exitCode: null, signal: 'SIGTERM', stdout: '', stderr: '' })
            )
          })
        : originalUpgrade(item, signal)
    const { updater, posts } = harness([npm])
    await updater.check()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
    })
    await updater.updateAll()
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('updating')
    })
    await expect(updater.cancel()).resolves.toEqual({ canceled: true })
    await vi.waitFor(() => {
      const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
      expect(state?.phase).toBe('done')
      expect(state?.summary).toEqual({ updated: 0, failed: 0, skipped: 3, elapsedMs: expect.any(Number) })
    })
    await expect(updater.cancel()).resolves.toEqual({ canceled: false })
  })
})

describe('Updater.setManagers / snapshot / sessions', () => {
  it('persists enabled managers and broadcasts', async () => {
    const npm = fakeAdapter('npm')
    const brew = fakeAdapter('brew')
    const { updater, posts, storage } = harness([npm, brew], { sessions: ['tok-1'] })
    await expect(updater.setManagers(['brew'])).resolves.toEqual({ ok: true })
    expect(storage.get('managers')).toEqual(['brew'])
    const state = posts.filter((post) => post.topic === 'state').at(-1)?.payload as UpdaterState
    expect(state.managers.map((manager) => ({ id: manager.id, enabled: manager.enabled }))).toEqual([
      { id: 'npm', enabled: false },
      { id: 'brew', enabled: true }
    ])
    await expect(updater.setManagers(['nope'])).resolves.toEqual({
      ok: false,
      error: 'unknown_manager',
      manager: 'nope'
    })
  })

  it('restores persisted manager selection and reports counts as null before a check', async () => {
    const npm = fakeAdapter('npm')
    const brew = fakeAdapter('brew')
    const { updater, storage } = harness([npm, brew])
    storage.set('managers', ['npm'])
    const snapshot = await updater.snapshot()
    expect(snapshot.managers.map((manager) => manager.enabled)).toEqual([true, false])
    expect(snapshot.state.checkedAt).toBeNull()
    expect(snapshot.managers.every((manager) => manager.count === null)).toBe(true)
  })

  it('prunes sessions that report unavailable and stops posting to them', async () => {
    const npm = fakeAdapter('npm')
    const { updater, posts, failingTokens } = harness([npm], { sessions: ['dead', 'live'] })
    failingTokens.add('dead')
    await updater.setManagers(['npm'])
    expect(posts.every((post) => post.token === 'live')).toBe(true)
    await updater.setManagers(['npm'])
    expect(posts.every((post) => post.token === 'live')).toBe(true)
    expect(posts.length).toBeGreaterThan(0)
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run plugins/updater/src/worker/jobs.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现（`src/worker/jobs.ts`）**

```ts
import type { ManagerAdapter } from './managers'
import type {
  JobPhase,
  ManagerId,
  ManagerState,
  OutdatedEntry,
  UpdateRow,
  UpdaterState,
  UpdateSummary
} from '../shared/types'
import {
  LOG_FLUSH_INTERVAL_MS,
  MANAGERS_STORAGE_KEY,
  MAX_LOG_CHUNK_BYTES,
  MAX_LOG_LINE_CHARS
} from '../shared/types'

export type UpdaterDeps = {
  adapters: ManagerAdapter[]
  storageGet: (key: string) => Promise<unknown>
  storageSet: (key: string, value: unknown) => Promise<void>
  postTo: (sessionToken: string, topic: 'state' | 'log', payload: unknown) => Promise<void>
  now: () => number
  /** Broadcast coalescing window; 0 flushes synchronously (tests). */
  stateCoalesceMs?: number
  /** Log flush interval; 0 flushes per line (tests). */
  logFlushMs?: number
  setIntervalFn?: (fn: () => void, ms: number) => unknown
  clearIntervalFn?: (handle: unknown) => void
}

export type SetManagersResult =
  | { ok: true }
  | { ok: false; error: 'unknown_manager'; manager: string }

type RunningJob = { abort: AbortController }

/**
 * Single-flight updater job machine: detect → check → per-package sequential
 * update with live pushes to every session that has ever invoked a command.
 */
export class Updater {
  private readonly deps: UpdaterDeps
  private readonly stateCoalesceMs: number
  private readonly logFlushMs: number
  private phase: JobPhase = 'idle'
  private checkedAt: number | null = null
  private rows: UpdateRow[] = []
  private summary: UpdateSummary | undefined
  private error: string | undefined
  private detected = new Map<ManagerId, boolean>()
  private counts = new Map<ManagerId, number | null>()
  private managerErrors = new Map<ManagerId, string>()
  private enabledIds: Set<string> | null = null
  private running: RunningJob | null = null
  private sessions = new Set<string>()
  private logBuffer: string[] = []
  private flushHandle: unknown = null
  private stateTimer: ReturnType<typeof setTimeout> | null = null
  private lastStateAt = 0
  private loaded = false

  constructor(deps: UpdaterDeps) {
    this.deps = deps
    this.stateCoalesceMs = deps.stateCoalesceMs ?? 250
    this.logFlushMs = deps.logFlushMs ?? LOG_FLUSH_INTERVAL_MS
  }

  rememberSession(token: string): void {
    if (token.length > 0) {
      this.sessions.add(token)
    }
  }

  async snapshot(): Promise<{ managers: ManagerState[]; state: UpdaterState }> {
    await this.ensureLoaded()
    return { managers: this.buildManagerStates(), state: this.buildState() }
  }

  async check(): Promise<{ started: boolean; running: boolean }> {
    await this.ensureLoaded()
    if (this.phase === 'checking' || this.phase === 'updating') {
      this.broadcastState()
      return { started: false, running: true }
    }
    void this.runCheck()
    return { started: true, running: false }
  }

  async updateAll(): Promise<{ started: boolean; running: boolean }> {
    await this.ensureLoaded()
    if (this.running) {
      this.broadcastState()
      return { started: false, running: true }
    }
    void this.runUpdate()
    return { started: true, running: false }
  }

  async cancel(): Promise<{ canceled: boolean }> {
    if (!this.running) {
      return { canceled: false }
    }
    this.running.abort.abort()
    return { canceled: true }
  }

  async setManagers(ids: string[]): Promise<SetManagersResult> {
    await this.ensureLoaded()
    const known = new Set(this.deps.adapters.map((adapter) => adapter.id as string))
    for (const id of ids) {
      if (!known.has(id)) {
        return { ok: false, error: 'unknown_manager', manager: id }
      }
    }
    const unique = [...new Set(ids)]
    await this.deps.storageSet(MANAGERS_STORAGE_KEY, unique)
    this.enabledIds = new Set(unique)
    this.broadcastState()
    return { ok: true }
  }

  private async ensureLoaded(): Promise<void> {
    if (this.loaded) {
      return
    }
    let stored: unknown = null
    try {
      stored = await this.deps.storageGet(MANAGERS_STORAGE_KEY)
    } catch {
      stored = null
    }
    if (Array.isArray(stored)) {
      const known = new Set(this.deps.adapters.map((adapter) => adapter.id as string))
      this.enabledIds = new Set(stored.filter((id): id is string => typeof id === 'string' && known.has(id)))
    }
    this.loaded = true
  }

  private isEnabled(id: ManagerId, detected: boolean): boolean {
    if (this.enabledIds !== null) {
      return this.enabledIds.has(id)
    }
    return detected
  }

  private activeAdapters(): ManagerAdapter[] {
    return this.deps.adapters.filter((adapter) => {
      const detected = this.detected.get(adapter.id) ?? false
      return detected && this.isEnabled(adapter.id, detected)
    })
  }

  private buildManagerStates(): ManagerState[] {
    return this.deps.adapters.map((adapter) => {
      const detected = this.detected.get(adapter.id) ?? false
      const error = this.managerErrors.get(adapter.id)
      return {
        id: adapter.id,
        label: adapter.label,
        detected,
        enabled: this.isEnabled(adapter.id, detected),
        count: this.counts.get(adapter.id) ?? null,
        ...(error !== undefined ? { error } : {})
      }
    })
  }

  private buildState(): UpdaterState {
    return {
      phase: this.phase,
      checkedAt: this.checkedAt,
      managers: this.buildManagerStates(),
      rows: this.rows,
      ...(this.summary !== undefined ? { summary: this.summary } : {}),
      ...(this.error !== undefined ? { error: this.error } : {})
    }
  }

  private async runCheck(): Promise<void> {
    this.phase = 'checking'
    this.error = undefined
    this.summary = undefined
    this.counts.clear()
    this.managerErrors.clear()
    await Promise.all(
      this.deps.adapters.map(async (adapter) => {
        this.detected.set(adapter.id, await adapter.detect())
      })
    )
    this.broadcastState()

    const rows: OutdatedEntry[] = []
    const active = this.activeAdapters()
    let failures = 0
    for (const adapter of active) {
      try {
        const result = await adapter.outdated()
        this.counts.set(adapter.id, result.entries.length)
        rows.push(...result.entries)
      } catch (error) {
        failures += 1
        this.managerErrors.set(adapter.id, messageOf(error))
        this.counts.set(adapter.id, null)
      }
      this.broadcastState()
    }
    this.rows = rows
      .map((row): UpdateRow => ({ ...row, state: 'pending' }))
      .sort((a, b) => a.manager.localeCompare(b.manager) || a.name.localeCompare(b.name))
    this.checkedAt = this.deps.now()
    this.phase = active.length > 0 && failures === active.length ? 'failed' : 'done'
    if (this.phase === 'failed') {
      this.error = active.map((adapter) => this.managerErrors.get(adapter.id)).filter(Boolean).join('; ')
    }
    this.broadcastState()
  }

  private async runUpdate(): Promise<void> {
    if (this.checkedAt === null || this.rows.every((row) => row.state !== 'pending')) {
      await this.runCheck()
    }
    const pending = this.rows.filter((row) => row.state === 'pending')
    if (pending.length === 0) {
      this.phase = 'done'
      this.broadcastState()
      return
    }
    const running: RunningJob = { abort: new AbortController() }
    this.running = running
    this.phase = 'updating'
    this.summary = undefined
    this.error = undefined
    this.broadcastState()
    const startedAt = this.deps.now()
    let updated = 0
    let failed = 0
    let skipped = 0
    for (const row of this.rows) {
      if (row.state !== 'pending') {
        continue
      }
      if (running.abort.signal.aborted) {
        row.state = 'skipped'
        skipped += 1
        continue
      }
      const adapter = this.deps.adapters.find((candidate) => candidate.id === row.manager)
      if (adapter === undefined) {
        row.state = 'failed'
        row.error = 'manager is not registered'
        failed += 1
        this.broadcastState()
        continue
      }
      row.state = 'running'
      this.broadcastState()
      this.pushLogLine(`$ ${adapter.id} ${adapter.upgradeArgs(row).join(' ')}`)
      try {
        const result = await adapter.upgrade(row, running.abort.signal)
        if (result.exitCode === 0) {
          row.state = 'updated'
          updated += 1
          this.pushLogLine(`ok: ${row.name} ${row.latest}`)
        } else if (running.abort.signal.aborted) {
          row.state = 'skipped'
          skipped += 1
          this.pushLogLine(`cancelled: ${row.name}`)
        } else {
          const tail = (result.stderr.trim() || result.stdout.trim()).split('\n').slice(-3).join('\n')
          row.state = 'failed'
          row.error = tail || `exit ${result.exitCode}`
          failed += 1
          this.pushLogLine(`error: ${row.name}: ${row.error}`)
        }
      } catch (error) {
        row.state = 'failed'
        row.error = messageOf(error)
        failed += 1
        this.pushLogLine(`error: ${row.name}: ${row.error}`)
      }
      this.broadcastState()
    }
    this.summary = { updated, failed, skipped, elapsedMs: this.deps.now() - startedAt }
    this.phase = 'done'
    this.running = null
    this.broadcastState()
    this.stopLogFlush()
  }

  private broadcastState(): void {
    if (this.stateCoalesceMs === 0) {
      this.flushState()
      return
    }
    const now = this.deps.now()
    const elapsed = now - this.lastStateAt
    if (elapsed >= this.stateCoalesceMs) {
      this.flushState()
      return
    }
    if (this.stateTimer === null) {
      this.stateTimer = setTimeout(() => {
        this.stateTimer = null
        this.flushState()
      }, this.stateCoalesceMs - elapsed)
    }
  }

  private flushState(): void {
    this.lastStateAt = this.deps.now()
    const state = this.buildState()
    for (const token of [...this.sessions]) {
      void this.postTo(token, 'state', state)
    }
  }

  private pushLogLine(line: string): void {
    const bounded = line.length > MAX_LOG_LINE_CHARS ? line.slice(0, MAX_LOG_LINE_CHARS) : line
    this.logBuffer.push(bounded)
    const bytes = this.logBuffer.reduce((total, entry) => total + entry.length + 1, 0)
    if (this.logFlushMs === 0 || bytes >= MAX_LOG_CHUNK_BYTES || this.logBuffer.length >= 200) {
      this.flushLogs()
      return
    }
    if (this.flushHandle === null) {
      const setIntervalFn =
        this.deps.setIntervalFn ?? ((fn: () => void, ms: number) => setInterval(fn, ms))
      this.flushHandle = setIntervalFn(() => this.flushLogs(), this.logFlushMs)
    }
  }

  private flushLogs(): void {
    if (this.logBuffer.length === 0) {
      return
    }
    const lines = this.logBuffer
    this.logBuffer = []
    for (const token of [...this.sessions]) {
      void this.postTo(token, 'log', { lines })
    }
  }

  private stopLogFlush(): void {
    if (this.flushHandle !== null) {
      const clearIntervalFn =
        this.deps.clearIntervalFn ?? ((handle: unknown) => clearInterval(handle as NodeJS.Timeout))
      clearIntervalFn(this.flushHandle)
      this.flushHandle = null
    }
    this.flushLogs()
  }

  private async postTo(token: string, topic: 'state' | 'log', payload: unknown): Promise<void> {
    try {
      await this.deps.postTo(token, topic, payload)
    } catch (error) {
      if ((error as { code?: unknown })?.code === 'unavailable') {
        this.sessions.delete(token)
      }
    }
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
```

- [ ] **Step 4: 运行通过**

Run: `pnpm vitest run plugins/updater/src/worker/jobs.test.ts`
Expected: PASS（单飞/取消/失败隔离/会话剔除用例全绿）

- [ ] **Step 5: 提交**

```bash
git add plugins/updater/src/shared/types.ts plugins/updater/src/worker/jobs.ts plugins/updater/src/worker/jobs.test.ts
git commit -m "feat(updater): 单飞任务状态机与推送/预算控制（T14）"
```

## Task 15: worker 命令接线（`main.ts`）

**Files:**
- Modify: `plugins/updater/src/worker/main.ts`（替换 T10 占位）
- Test: `plugins/updater/src/worker/main.test.ts`

**Interfaces:**
- Consumes: T14 `Updater`。
- Produces: `OrcaSdk` 类型、`createUpdater(orca, overrides?)`、`registerCommands(orca, updater)`；`activate` 注册 `check` / `update-all` / `cancel` / `open` / `snapshot` / `set-managers` 六个命令。

- [ ] **Step 1: 写失败测试**

`src/worker/main.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import { createUpdater, registerCommands, type CommandContext, type OrcaSdk } from './main'

type Handler = (args: unknown, ctx?: CommandContext) => unknown

function createFakeOrca() {
  const handlers = new Map<string, Handler>()
  const hostCalls: Array<{ method: string; params: unknown }> = []
  const orca: OrcaSdk = {
    commands: {
      register: (id, handler) => {
        handlers.set(id, handler)
      }
    },
    host: {
      call: async (method, params) => {
        hostCalls.push({ method, params })
        if (method === 'storage.get') return { value: null }
        if (method === 'storage.set') return { ok: true }
        if (method === 'session.post') return { ok: true }
        if (method === 'workspace.openTab') return { instanceId: 'i1', worktreeId: 'wt' }
        throw new Error(`unexpected host call: ${method}`)
      }
    },
    log: () => {},
    events: { on: () => {} }
  }
  return { orca, handlers, hostCalls }
}

describe('registerCommands', () => {
  it('registers the six updater commands and remembers the calling session', async () => {
    const { orca, handlers, hostCalls } = createFakeOrca()
    const updater = {
      rememberSession: vi.fn(),
      check: vi.fn(async () => ({ started: true, running: false })),
      updateAll: vi.fn(async () => ({ started: true, running: false })),
      cancel: vi.fn(async () => ({ canceled: false })),
      snapshot: vi.fn(async () => ({ managers: [], state: {} })),
      setManagers: vi.fn(async () => ({ ok: true as const }))
    }
    registerCommands(orca, updater as never)
    expect([...handlers.keys()].sort()).toEqual([
      'cancel',
      'check',
      'open',
      'set-managers',
      'snapshot',
      'update-all'
    ])

    const ctx: CommandContext = {
      session: { token: 'tok-1', post: async () => ({ ok: true }) }
    }
    await handlers.get('check')!(undefined, ctx)
    expect(updater.rememberSession).toHaveBeenCalledWith('tok-1')
    expect(updater.check).toHaveBeenCalled()

    await handlers.get('set-managers')!({ managers: ['npm', 42] }, ctx)
    expect(updater.setManagers).toHaveBeenCalledWith(['npm'])

    await handlers.get('open')!(undefined, ctx)
    expect(hostCalls.some((call) => call.method === 'workspace.openTab')).toBe(true)
  })
})

describe('createUpdater', () => {
  it('routes storage through the orca host API', async () => {
    const { orca, hostCalls } = createFakeOrca()
    const updater = createUpdater(orca)
    await expect(updater.setManagers(['npm'])).resolves.toEqual({ ok: true })
    expect(
      hostCalls.some(
        (call) => call.method === 'storage.set' && (call.params as { key?: string }).key === 'managers'
      )
    ).toBe(true)
  })
})
```

（`open` 命令的 `workspace.openTab` 断言并入 `registerCommands` 测试其余分支即可；`hostCalls` 由 createFakeOrca 暴露。）

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run plugins/updater/src/worker/main.test.ts`
Expected: FAIL（导出不存在）

- [ ] **Step 3: 实现（`src/worker/main.ts` 全量替换）**

```ts
import { Updater, type UpdaterDeps } from './jobs'
import { defaultAdapters } from './managers'

export type SessionContext = {
  token: string
  post: (topic: string, payload: unknown) => Promise<unknown>
}

export type CommandContext = { session?: SessionContext }

export type OrcaSdk = {
  commands: { register: (id: string, handler: (args: unknown, ctx?: CommandContext) => unknown) => void }
  host: { call: (method: string, params: unknown) => Promise<unknown> }
  log: (message: string) => void
  events: { on: (event: string, handler: (payload: unknown) => void) => void }
}

export function createUpdater(orca: OrcaSdk, overrides: Partial<UpdaterDeps> = {}): Updater {
  return new Updater({
    adapters: defaultAdapters(),
    storageGet: async (key) => {
      const result = (await orca.host.call('storage.get', { key })) as { value?: unknown }
      return result?.value ?? null
    },
    storageSet: async (key, value) => {
      await orca.host.call('storage.set', { key, value })
    },
    postTo: async (sessionToken, topic, payload) => {
      await orca.host.call('session.post', { sessionToken, topic, payload })
    },
    now: () => Date.now(),
    ...overrides
  })
}

export function registerCommands(orca: OrcaSdk, updater: Updater): void {
  const withSession =
    <T>(run: () => Promise<T>) =>
    (_args: unknown, ctx?: CommandContext): Promise<T> => {
      if (ctx?.session) {
        updater.rememberSession(ctx.session.token)
      }
      return run()
    }

  orca.commands.register('check', withSession(() => updater.check()))
  orca.commands.register('update-all', withSession(() => updater.updateAll()))
  orca.commands.register('cancel', withSession(() => updater.cancel()))
  orca.commands.register('snapshot', withSession(() => updater.snapshot()))
  orca.commands.register('set-managers', (args, ctx) => {
    if (ctx?.session) {
      updater.rememberSession(ctx.session.token)
    }
    const managers = (args as { managers?: unknown } | undefined)?.managers
    const ids = Array.isArray(managers)
      ? managers.filter((id): id is string => typeof id === 'string')
      : []
    return updater.setManagers(ids)
  })
  orca.commands.register('open', (_args, ctx) => {
    if (ctx?.session) {
      updater.rememberSession(ctx.session.token)
    }
    return orca.host.call('workspace.openTab', { tabId: 'updates' })
  })
}

export default function activate(orca: OrcaSdk): void {
  registerCommands(orca, createUpdater(orca))
  orca.log('updater activated')
}
```

- [ ] **Step 4: 运行通过**

Run: `pnpm vitest run plugins/updater/src/worker/main.test.ts`
Expected: PASS

- [ ] **Step 5: 构建冒烟（产物可被 node 加载且无缺依赖）**

Run: `pnpm build:updater && node --check plugins/updater/dist/main.mjs`
Expected: exit 0

- [ ] **Step 6: 提交**

```bash
git add plugins/updater/src/worker/main.ts plugins/updater/src/worker/main.test.ts
git commit -m "feat(updater): 六命令接线与宿主桥装配（T15）"
```

## Task 16: UI 基建（主题 parity / kit / 桥 / 状态）

**Files:**
- Modify: `plugins/updater/src/ui/theme.css`（替换 T10 占位为完整版）
- Create: `plugins/updater/src/ui/theme-parity.test.ts`
- Create: `plugins/updater/src/ui/kit.tsx`
- Create: `plugins/updater/src/ui/bridge.ts`
- Create: `plugins/updater/src/ui/state.ts`
- Test: `plugins/updater/src/ui/state.test.ts`

**Interfaces:**
- Produces: `HostBridge`（`ready` / `invokeCommand` / `onPush` / `onInit`）、`createHostBridge()`、`reducePush(state, message)`、`UiState`、`Button` / `Chip` / `Toggle` / `Skeleton`。供 T17 使用。

- [ ] **Step 1: 写 parity 失败测试**

`src/ui/theme-parity.test.ts`：

```ts
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const hostCss = readFileSync('src/renderer/src/assets/main.css', 'utf8')
const pluginCss = readFileSync('plugins/updater/src/ui/theme.css', 'utf8')

function hostThemeBlock(): string {
  const match = hostCss.match(/@theme inline \{[\s\S]*?\n\}/)
  if (!match) {
    throw new Error('host main.css has no @theme inline block')
  }
  return match[0]
}

function parityRegion(): string {
  const match = pluginCss.match(
    /\/\* plugin-theme-parity:begin \*\/([\s\S]*?)\/\* plugin-theme-parity:end \*\//
  )
  if (!match) {
    throw new Error('plugin theme.css has no parity region')
  }
  return match[1]
}

describe('host theme parity', () => {
  it('keeps the host dark variant and @theme mapping verbatim', () => {
    const region = parityRegion()
    expect(region).toContain('@custom-variant dark (&:is(.dark *))')
    expect(region).toContain(hostThemeBlock())
  })

  it('inlines the host Geist font beside the mapping', () => {
    const region = parityRegion()
    expect(region).toContain(
      "url('../../../../src/renderer/src/assets/fonts/Geist-Variable.woff2')"
    )
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run plugins/updater/src/ui/theme-parity.test.ts`
Expected: FAIL（无 parity 区）

- [ ] **Step 3: 实现 theme.css（完整替换）**

把宿主 `src/renderer/src/assets/main.css` 中的两段原文**逐字复制**进 parity 区：① `@custom-variant dark (&:is(.dark *))` 行；② `@theme inline { … }` 整块（自 `@theme inline {` 到其闭合 `}`；不含注释）。成品结构：

```css
@import 'tailwindcss';

/* The region below mirrors the host theme verbatim; theme-parity.test.ts
   extracts and compares it, so edits must be synced with
   src/renderer/src/assets/main.css. */
/* plugin-theme-parity:begin */
@font-face {
  font-family: 'Geist';
  src: url('../../../../src/renderer/src/assets/fonts/Geist-Variable.woff2') format('woff2');
  font-weight: 100 900;
  font-style: normal;
  font-display: swap;
}

@custom-variant dark (&:is(.dark *));

@theme inline {
  /* …宿主 main.css 的 @theme inline 块逐字复制于此… */
}
/* plugin-theme-parity:end */

:root {
  --app-font-family: 'Geist', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  --font-sans: var(--app-font-family);
  --font-mono:
    'SF Mono', SFMono-Regular, ui-monospace, 'Cascadia Code', Menlo, Consolas,
    'Liberation Mono', monospace;
}

html,
body {
  height: 100%;
}

body {
  margin: 0;
  font-family: var(--font-sans);
  font-size: 13px;
  line-height: 1.45;
  color: var(--foreground);
  background: var(--background);
}
```

（`@theme inline` 块内注释不复制——parity 测试按提取串逐字比对，宿主块无注释，插件块必须一致。）

- [ ] **Step 4: 运行 parity 通过**

Run: `pnpm vitest run plugins/updater/src/ui/theme-parity.test.ts`
Expected: PASS

- [ ] **Step 5: 状态归并测试（`src/ui/state.test.ts`）**

```ts
import { describe, expect, it } from 'vitest'
import { INITIAL_UI_STATE, reducePush } from './state'

describe('reducePush', () => {
  it('replaces the report on state pushes', () => {
    const next = reducePush(INITIAL_UI_STATE, {
      type: 'orca-panel-push',
      topic: 'state',
      payload: { phase: 'done', checkedAt: 1, managers: [], rows: [] }
    })
    expect(next.report).toEqual({ phase: 'done', checkedAt: 1, managers: [], rows: [] })
  })

  it('appends log lines and caps the buffer', () => {
    let state = INITIAL_UI_STATE
    for (let index = 0; index < 3; index += 1) {
      state = reducePush(state, {
        type: 'orca-panel-push',
        topic: 'log',
        payload: { lines: [`line-${index}`] }
      })
    }
    expect(state.logs).toEqual(['line-0', 'line-1', 'line-2'])
    expect(reducePush(state, { type: 'orca-panel-push', payload: 1 })).toBe(state)
  })
})
```

- [ ] **Step 6: 实现 state / kit / bridge**

`src/ui/state.ts`：

```ts
import type { PluginPanelPushMessage } from '../../../../shared/plugins/plugin-panel-bridge'
import type { UpdaterState } from '../shared/types'

export type UiState = {
  report: UpdaterState | null
  logs: string[]
}

export const INITIAL_UI_STATE: UiState = { report: null, logs: [] }

const MAX_LOG_LINES = 500

export function reducePush(state: UiState, message: PluginPanelPushMessage): UiState {
  if (message.topic === 'log') {
    const lines = (message.payload as { lines?: unknown } | null)?.lines
    if (!Array.isArray(lines)) {
      return state
    }
    const appended = lines.filter((line): line is string => typeof line === 'string')
    return { ...state, logs: [...state.logs, ...appended].slice(-MAX_LOG_LINES) }
  }
  if (message.topic === 'state') {
    return { ...state, report: message.payload as UpdaterState }
  }
  return state
}
```

`src/ui/kit.tsx`：

```tsx
import type { ButtonHTMLAttributes, ReactNode } from 'react'

export function buttonClass(
  variant: 'default' | 'primary' | 'danger' = 'default',
  disabled = false
): string {
  const base =
    'inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-xs font-medium transition-colors'
  if (disabled) {
    return `${base} border-border bg-muted text-muted-foreground`
  }
  if (variant === 'primary') {
    return `${base} border-primary bg-primary text-primary-foreground hover:opacity-90`
  }
  if (variant === 'danger') {
    return `${base} border-destructive bg-transparent text-destructive hover:bg-destructive/10`
  }
  return `${base} border-border bg-background text-foreground hover:bg-accent`
}

export function Button({
  variant = 'default',
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: 'default' | 'primary' | 'danger' }) {
  return (
    <button type="button" className={`${buttonClass(variant, props.disabled === true)} ${className ?? ''}`} {...props} />
  )
}

export function Chip({
  selected = false,
  children,
  onClick
}: {
  selected?: boolean
  children: ReactNode
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        selected
          ? 'rounded-md bg-primary px-2 py-0.5 text-[11px] text-primary-foreground'
          : 'rounded-md bg-muted px-2 py-0.5 text-[11px] text-muted-foreground hover:text-foreground'
      }
    >
      {children}
    </button>
  )
}

export function Toggle({
  checked,
  onChange,
  label
}: {
  checked: boolean
  onChange: (next: boolean) => void
  label: string
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-[18px] w-[30px] shrink-0 rounded-full transition-colors ${checked ? 'bg-primary' : 'bg-border'}`}
    >
      <span
        className={`absolute top-[2px] h-[14px] w-[14px] rounded-full bg-background shadow transition-all ${checked ? 'left-[14px]' : 'left-[2px]'}`}
      />
    </button>
  )
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={`animate-pulse rounded-md bg-muted ${className ?? ''}`} />
}
```

`src/ui/bridge.ts`：

```ts
import {
  PANEL_ACTION_REQUEST_TYPE,
  PANEL_ACTION_RESULT_TYPE,
  PANEL_INIT_TYPE,
  PANEL_PONG_TYPE,
  PANEL_PUSH_TYPE,
  parsePanelInitMessage,
  parsePanelPushMessage,
  type PluginPanelPushMessage
} from '../../../../shared/plugins/plugin-panel-bridge'

export type PanelActionResult =
  | { ok: true; value: unknown }
  | { ok: false; errorCode?: string; error?: string }

export type HostBridge = {
  ready: Promise<void>
  invokeCommand: (commandId: string, args?: unknown) => Promise<PanelActionResult>
  onPush: (listener: (message: PluginPanelPushMessage) => void) => void
  onInit: (listener: (params: unknown) => void) => void
}

export function createHostBridge(): HostBridge {
  const pushListeners: Array<(message: PluginPanelPushMessage) => void> = []
  const initListeners: Array<(params: unknown) => void> = []
  const pending = new Map<string, (result: PanelActionResult) => void>()
  let sequence = 0
  let connected = false
  let resolveReady: () => void = () => {}
  const ready = new Promise<void>((resolve) => {
    resolveReady = resolve
  })

  window.addEventListener('message', (event) => {
    const data = event.data as Record<string, unknown> | null
    if (!data || typeof data !== 'object') {
      return
    }
    if (data.type === PANEL_PONG_TYPE && !connected) {
      connected = true
      resolveReady()
      return
    }
    if (data.type === PANEL_ACTION_RESULT_TYPE) {
      if (typeof data.requestId !== 'string') {
        return
      }
      const resolve = pending.get(data.requestId)
      if (!resolve) {
        return
      }
      pending.delete(data.requestId)
      resolve(
        data.ok === true
          ? { ok: true, value: data.value }
          : {
              ok: false,
              errorCode: typeof data.errorCode === 'string' ? data.errorCode : undefined,
              error: typeof data.error === 'string' ? data.error : undefined
            }
      )
      return
    }
    if (data.type === PANEL_PUSH_TYPE) {
      const parsed = parsePanelPushMessage(data)
      if (parsed.ok) {
        for (const listener of pushListeners) {
          listener(data as PluginPanelPushMessage)
        }
      }
      return
    }
    if (data.type === PANEL_INIT_TYPE) {
      const parsed = parsePanelInitMessage(data)
      if (parsed.ok) {
        for (const listener of initListeners) {
          listener(parsed.params)
        }
      }
    }
  })

  const call = (action: string, params?: unknown): Promise<PanelActionResult> =>
    new Promise((resolve) => {
      const requestId = `req-${++sequence}`
      pending.set(requestId, resolve)
      window.parent.postMessage(
        { type: PANEL_ACTION_REQUEST_TYPE, requestId, action, params },
        '*'
      )
    })

  return {
    ready,
    invokeCommand: (commandId, args) => call('worker.invokeCommand', { commandId, args }),
    onPush: (listener) => {
      pushListeners.push(listener)
    },
    onInit: (listener) => {
      initListeners.push(listener)
    }
  }
}
```

- [ ] **Step 7: 运行并提交**

Run: `pnpm vitest run plugins/updater/src/ui` 与 `pnpm typecheck`
Expected: PASS / exit 0

```bash
git add plugins/updater/src/ui
git commit -m "feat(updater): UI 基建（主题 parity/kit/桥/状态）（T16）"
```

## Task 17: UI 视图（tab + panel 单文件入口）

**Files:**
- Modify: `plugins/updater/src/ui/tab/main.tsx`、`plugins/updater/src/ui/panel/main.tsx`（替换占位）
- Create: `plugins/updater/src/ui/tab/App.tsx`
- Create: `plugins/updater/src/ui/panel/App.tsx`
- Test: `plugins/updater/src/ui/tab/App.test.tsx`、`plugins/updater/src/ui/panel/App.test.tsx`

**Interfaces:**
- Consumes: T14 的 `UpdaterState`/`ManagerState`、T16 的 `HostBridge`/`UiState`/`reducePush`/kit。
- Produces: `TabApp({bridge})`、`PanelApp({bridge})`；两个入口挂载真实 `createHostBridge()`。

- [ ] **Step 1: 写 tab 渲染测试（失败）**

`src/ui/tab/App.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { HostBridge } from '../bridge'
import type { PluginPanelPushMessage } from '../../../../../shared/plugins/plugin-panel-bridge'
import { TabApp } from './App'

function fakeBridge() {
  const pushListeners: Array<(message: PluginPanelPushMessage) => void> = []
  const invokeCommand = vi.fn(async () => ({ ok: true as const, value: null }))
  const bridge = {
    ready: Promise.resolve(),
    invokeCommand,
    onPush: (listener: (message: PluginPanelPushMessage) => void) => {
      pushListeners.push(listener)
    },
    onInit: () => {}
  } as unknown as HostBridge
  return { bridge, invokeCommand, pushListeners }
}

let root: Root | null = null
let container: HTMLDivElement | null = null

afterEach(() => {
  act(() => root?.unmount())
  container?.remove()
  root = null
  container = null
})

describe('TabApp', () => {
  it('checks on mount and renders pushed rows with the live output pane', async () => {
    const { bridge, invokeCommand, pushListeners } = fakeBridge()
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    await act(async () => {
      root!.render(<TabApp bridge={bridge} />)
      await Promise.resolve()
    })
    expect(invokeCommand).toHaveBeenCalledWith('check')

    await act(async () => {
      for (const listener of pushListeners) {
        listener({
          type: 'orca-panel-push',
          topic: 'state',
          payload: {
            phase: 'done',
            checkedAt: Date.now(),
            managers: [
              { id: 'npm', label: 'npm', detected: true, enabled: true, count: 2 }
            ],
            rows: [
              { manager: 'npm', kind: 'global', name: 'typescript', current: '5.8.3', latest: '5.9.2', state: 'pending' },
              { manager: 'npm', kind: 'global', name: 'eslint', current: '9.18.0', latest: '9.21.0', state: 'pending' }
            ]
          }
        })
        listener({
          type: 'orca-panel-push',
          topic: 'log',
          payload: { lines: ['$ npm install -g typescript@latest'] }
        })
      }
    })

    const text = container.textContent ?? ''
    expect(text).toContain('typescript')
    expect(text).toContain('5.9.2')
    expect(text).toContain('Update all (2)')
    expect(text).toContain('npm install -g typescript@latest')
  })
})
```

`src/ui/panel/App.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { HostBridge } from '../bridge'
import { PanelApp } from './App'

let root: Root | null = null
let container: HTMLDivElement | null = null

afterEach(() => {
  act(() => root?.unmount())
  container?.remove()
  root = null
  container = null
})

describe('PanelApp', () => {
  it('loads a snapshot and persists manager toggles through set-managers', async () => {
    const invokeCommand = vi.fn(async (commandId: string) => {
      if (commandId === 'snapshot') {
        return {
          ok: true as const,
          value: {
            managers: [
              { id: 'npm', label: 'npm', detected: true, enabled: true, count: 4 },
              { id: 'brew', label: 'Homebrew', detected: false, enabled: false, count: null }
            ],
            state: { phase: 'done', checkedAt: null, managers: [], rows: [] }
          }
        }
      }
      return { ok: true as const, value: null }
    })
    const bridge = {
      ready: Promise.resolve(),
      invokeCommand,
      onPush: () => {},
      onInit: () => {}
    } as unknown as HostBridge
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    await act(async () => {
      root!.render(<PanelApp bridge={bridge} />)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(invokeCommand).toHaveBeenCalledWith('snapshot')
    expect(container.textContent).toContain('Homebrew')
    expect(container.textContent).toContain('not installed')

    const toggle = container.querySelector('[role="switch"]')
    expect(toggle).not.toBeNull()
    await act(async () => {
      toggle!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await Promise.resolve()
    })
    expect(invokeCommand).toHaveBeenCalledWith('set-managers', { managers: ['brew'] })
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run plugins/updater/src/ui/tab plugins/updater/src/ui/panel`
Expected: FAIL（App 不存在）

- [ ] **Step 3: 实现 tab 视图（`src/ui/tab/App.tsx`）**

```tsx
import { useEffect, useMemo, useState } from 'react'
import type { HostBridge } from '../bridge'
import { reducePush, type UiState } from '../state'
import { Button, Chip, Skeleton } from '../kit'
import type { UpdateRow, UpdaterState } from '../../shared/types'

function kindLabel(row: UpdateRow): string {
  if (row.manager === 'npm') {
    return 'npm'
  }
  return row.kind === 'cask' ? 'brew cask' : 'brew formula'
}

function formatCheckedAt(checkedAt: number | null): string {
  if (checkedAt === null) {
    return 'not checked'
  }
  const seconds = Math.max(0, Math.round((Date.now() - checkedAt) / 1000))
  return seconds < 60 ? `checked ${seconds}s ago` : `checked ${Math.round(seconds / 60)} min ago`
}

function stateLabel(row: UpdateRow): { text: string; className: string } {
  switch (row.state) {
    case 'updated':
      return { text: '✓ updated', className: 'text-foreground' }
    case 'running':
      return { text: 'running…', className: 'text-foreground' }
    case 'failed':
      return { text: '✗ failed', className: 'text-destructive' }
    case 'skipped':
      return { text: 'skipped', className: 'text-muted-foreground' }
    default:
      return { text: 'pending', className: 'text-muted-foreground' }
  }
}

export function TabApp({ bridge }: { bridge: HostBridge }) {
  const [ui, setUi] = useState<UiState>({ report: null, logs: [] })
  const [connected, setConnected] = useState(false)
  const [filter, setFilter] = useState<'all' | string>('all')
  const [actionError, setActionError] = useState<string | null>(null)

  useEffect(() => {
    void bridge.ready.then(() => {
      setConnected(true)
      bridge.onPush((message) => setUi((current) => reducePush(current, message)))
      void bridge.invokeCommand('check')
    })
  }, [bridge])

  const report = ui.report
  const running = report?.phase === 'checking' || report?.phase === 'updating'
  const pendingCount = report?.rows.filter((row) => row.state === 'pending').length ?? 0
  const managers = (report?.managers ?? []).filter((manager) => manager.enabled)

  const rows = useMemo(() => {
    const list = report?.rows ?? []
    return filter === 'all' ? list : list.filter((row) => row.manager === filter)
  }, [report, filter])

  const invoke = async (commandId: string) => {
    const result = await bridge.invokeCommand(commandId)
    setActionError(result.ok ? null : (result.error ?? result.errorCode ?? 'command failed'))
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-2 border-b border-border px-3.5 py-2.5">
        <span className="text-[13px] font-semibold">Updates</span>
        <span className="text-xs text-muted-foreground">{formatCheckedAt(report?.checkedAt ?? null)}</span>
        <div className="flex-1" />
        <Button disabled={running || !connected} onClick={() => void invoke('check')}>
          Refresh
        </Button>
        {running ? (
          <Button variant="danger" onClick={() => void invoke('cancel')}>
            Cancel
          </Button>
        ) : (
          <Button variant="primary" disabled={!connected || pendingCount === 0} onClick={() => void invoke('update-all')}>
            {`Update all (${pendingCount})`}
          </Button>
        )}
      </header>
      {actionError ? (
        <div className="border-b border-destructive/40 bg-destructive/10 px-3.5 py-1.5 text-xs text-destructive">
          {actionError}
        </div>
      ) : null}
      <div className="flex min-h-0 flex-1">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-1.5 border-b border-border px-3.5 py-2">
            <Chip selected={filter === 'all'} onClick={() => setFilter('all')}>
              {`All ${report?.rows.length ?? 0}`}
            </Chip>
            {managers.map((manager) => (
              <Chip key={manager.id} selected={filter === manager.id} onClick={() => setFilter(manager.id)}>
                {`${manager.label} ${manager.count ?? '—'}`}
              </Chip>
            ))}
          </div>
          <div className="min-h-0 flex-1 overflow-auto">{renderBody(report, rows, connected)}</div>
        </div>
        <aside className="flex w-[300px] shrink-0 flex-col border-l border-border">
          <div className="border-b border-border px-3.5 py-1.5 text-[11px] uppercase tracking-wide text-muted-foreground">
            Live output
          </div>
          <pre className="min-h-0 flex-1 overflow-auto px-3.5 py-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
            {ui.logs.length > 0 ? ui.logs.join('\n') : 'No output yet.'}
          </pre>
        </aside>
      </div>
    </div>
  )
}

function renderBody(report: UpdaterState | null, rows: UpdateRow[], connected: boolean) {
  if (!connected) {
    return <div className="p-10 text-center text-xs text-muted-foreground">Connecting…</div>
  }
  if (report === null || report.phase === 'checking') {
    return (
      <div className="space-y-2 p-3.5">
        {[0, 1, 2, 3].map((key) => (
          <Skeleton key={key} className="h-5" />
        ))}
      </div>
    )
  }
  if (report.phase === 'failed') {
    return <div className="p-3.5 text-xs text-destructive">{report.error ?? 'Check failed.'}</div>
  }
  if (rows.length === 0) {
    return <div className="p-10 text-center text-xs text-muted-foreground">All up to date</div>
  }
  return (
    <div>
      {report.summary ? (
        <div className="border-b border-border px-3.5 py-1.5 text-xs text-muted-foreground">
          {`✓ ${report.summary.updated} updated · ✗ ${report.summary.failed} failed · ${report.summary.skipped} skipped`}
        </div>
      ) : null}
      <table className="w-full border-collapse text-[12.5px]">
        <thead>
          <tr className="bg-muted text-left text-[11px] uppercase tracking-wide text-muted-foreground">
            <th className="px-3.5 py-1.5 font-semibold">package</th>
            <th className="px-3.5 py-1.5 font-semibold">source</th>
            <th className="px-3.5 py-1.5 font-semibold">version</th>
            <th className="px-3.5 py-1.5 text-right font-semibold">state</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const label = stateLabel(row)
            return (
              <tr key={`${row.manager}:${row.kind}:${row.name}`} className="border-b border-border">
                <td className="px-3.5 py-1.5">{row.name}</td>
                <td className="px-3.5 py-1.5 text-[11px] text-muted-foreground">{kindLabel(row)}</td>
                <td className="px-3.5 py-1.5 text-muted-foreground [font-variant-numeric:tabular-nums]">
                  {row.current} → <span className="font-medium text-foreground">{row.latest}</span>
                </td>
                <td className={`px-3.5 py-1.5 text-right text-[11px] ${label.className}`}>{label.text}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
```

- [ ] **Step 4: 实现 panel 视图（`src/ui/panel/App.tsx`）**

```tsx
import { useEffect, useState } from 'react'
import type { HostBridge } from '../bridge'
import { Button, Toggle } from '../kit'
import type { ManagerState } from '../../shared/types'

function managerSummary(manager: ManagerState): string {
  if (!manager.detected) {
    return 'not installed'
  }
  const scope = manager.id === 'npm' ? 'global' : 'formula · cask'
  const count = manager.count === null ? '—' : `${manager.count} updates`
  return `${scope} · ${count}`
}

function formatCheckedAt(checkedAt: number | null): string {
  if (checkedAt === null) {
    return 'not checked'
  }
  const seconds = Math.max(0, Math.round((Date.now() - checkedAt) / 1000))
  return seconds < 60 ? `${seconds}s ago` : `${Math.round(seconds / 60)} min ago`
}

export function PanelApp({ bridge }: { bridge: HostBridge }) {
  const [managers, setManagers] = useState<ManagerState[]>([])
  const [checkedAt, setCheckedAt] = useState<number | null>(null)
  const [connected, setConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void bridge.ready.then(() => {
      setConnected(true)
      bridge.onPush((message) => {
        if (message.topic !== 'state') {
          return
        }
        const state = message.payload as { managers: ManagerState[]; checkedAt: number | null }
        setManagers(state.managers)
        setCheckedAt(state.checkedAt)
      })
      void bridge
        .invokeCommand('snapshot')
        .then((result) => {
          if (!result.ok) {
            setError(result.error ?? result.errorCode ?? 'snapshot failed')
            return
          }
          const snapshot = result.value as { managers: ManagerState[]; state: { checkedAt: number | null } }
          setManagers(snapshot.managers)
          setCheckedAt(snapshot.state.checkedAt)
        })
    })
  }, [bridge])

  const toggle = async (manager: ManagerState, next: boolean) => {
    const previous = managers
    const nextIds = managers
      .filter((item) => (item.id === manager.id ? next : item.enabled))
      .map((item) => item.id)
    setManagers((current) =>
      current.map((item) => (item.id === manager.id ? { ...item, enabled: next } : item))
    )
    const result = await bridge.invokeCommand('set-managers', { managers: nextIds })
    if (result.ok) {
      setError(null)
    } else {
      setManagers(previous)
      setError(result.error ?? result.errorCode ?? 'could not save')
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-2 border-b border-border px-3 py-2.5">
        <span className="text-[13px] font-semibold">Package Updater</span>
        <div className="flex-1" />
        <span className="text-xs text-muted-foreground">{formatCheckedAt(checkedAt)}</span>
      </header>
      <div className="flex gap-2 border-b border-border px-3 py-2.5">
        <Button disabled={!connected} onClick={() => void bridge.invokeCommand('check')}>
          Check now
        </Button>
        <Button disabled={!connected} onClick={() => void bridge.invokeCommand('open')}>
          Open tab
        </Button>
      </div>
      {error ? (
        <div className="border-b border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          {error}
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-auto">
        {managers.map((manager) => (
          <div key={manager.id} className="flex items-center gap-2 border-b border-border px-3 py-2">
            <span
              className={`h-1.5 w-1.5 rounded-full ${manager.detected ? 'bg-foreground' : 'bg-muted-foreground'}`}
            />
            <div className="min-w-0 flex-1">
              <div className="text-[12.5px] font-medium">{manager.label}</div>
              <div className="text-[11px] text-muted-foreground">{managerSummary(manager)}</div>
            </div>
            <Toggle checked={manager.enabled} onChange={(next) => void toggle(manager, next)} label={manager.label} />
          </div>
        ))}
      </div>
      <div className="border-t border-border px-3 py-2 text-[11px] text-muted-foreground">
        No recent check. The tab checks automatically when opened.
      </div>
    </div>
  )
}
```

- [ ] **Step 5: 替换两个入口**

`src/ui/tab/main.tsx`：

```tsx
import { createRoot } from 'react-dom/client'
import '../theme.css'
import { createHostBridge } from '../bridge'
import { TabApp } from './App'

createRoot(document.getElementById('root') as HTMLElement).render(
  <TabApp bridge={createHostBridge()} />
)
```

`src/ui/panel/main.tsx`：同构，导入 `./App` 的 `PanelApp`。

- [ ] **Step 6: 运行 UI 测试 + 全插件测试 + 构建**

Run: `pnpm vitest run plugins/updater`
Expected: PASS（parse/runner/managers/jobs/main/state/parity/App 全部）
Run: `pnpm build:updater` + 单文件断言（T10 Step 6 的 node 命令）
Expected: `single-file ok`；`dist/tab.html` 含 `Geist`（data: URI 已内联：`grep -c "data:font" plugins/updater/dist/tab.html` ≥ 1）
Run: `pnpm typecheck`
Expected: exit 0

- [ ] **Step 7: 提交**

```bash
git add plugins/updater/src/ui
git commit -m "feat(updater): tab 与 panel 视图接入单文件入口（T17）"
```

## Task 18: 收尾（全量门禁 + 实施记录 + 手工验收清单）

**Files:**
- Create: `docs/phase3c-plugin-ui-worker-channel-record.md`
- Modify: 按需修复门禁暴露的问题

**Interfaces:**
- Produces: 与 Phase 3A 记录同构的实施记录（提交清单/门禁证据/手工验收状态/偏差备案），供用户做手工验收。

- [ ] **Step 1: 门禁一（Rust 全量）**

Run: `cargo test --workspace`（`src-tauri/`）
Expected: exit 0；失败则修复后重跑并记录

- [ ] **Step 2: 门禁二（bindings 新鲜度）**

Run: `cargo run -p ade-bridge --bin export-bindings`（`src-tauri/`）→ 仓库根 `git diff --exit-code -- src/bridge/real/generated/tauri-bindings.ts`
Expected: 零差异

- [ ] **Step 3: 门禁三（类型与构建）**

Run: `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web && pnpm build:updater`
Expected: 全部 exit 0

- [ ] **Step 4: 门禁四（TS 全量）**

Run: `pnpm test`
Expected: exit 0（含 `plugins/**` 单测与 parity；全量约 8–10 分钟）

- [ ] **Step 5: 写实施记录（`docs/phase3c-plugin-ui-worker-channel-record.md`）**

按 `docs/phase3a-plugin-system-record.md` 的章节结构落盘，必含：

1. 范围与验收表（按 A 部分/ B 部分列交付面与主提交 SHA）
2. 提交清单（`git log --oneline --reverse 8d8bb849..HEAD` 的筛选版本）
3. 门禁证据（四组命令的实测输出摘要与日志路径）
4. 手工验收清单（spec §5 的 11 条，标注"待用户复核"）
5. 规格偏差与边界备案（实施中新发现逐条；已知偏差：`set-managers` 结构化失败措辞修正、host-entry 与 app 同版发布假设等）
6. 已知边界与后续项（照抄 spec §7）

- [ ] **Step 6: 提交记录**

```bash
git add docs/phase3c-plugin-ui-worker-channel-record.md
git commit -m "docs: Phase 3C 实施记录与门禁证据（T18）"
```

---

## 执行注意

- **顺序**：A 部分 T1→T9 线性；B 部分 T10→T18 线性；B 依赖 A 的 T1/T3/T7（构建前完成）。
- **文件冲突**：T4/T5 依次编辑 `host/services.rs`、`host/methods.rs`、`service.rs`、`plugins_service.rs`，不得并行。
- **快速反馈**：每任务只跑该任务的测试；T18 一次性跑四组全量门禁。
- **spec 偏差即时记录**：任何与 `docs/superpowers/specs/2026-10-17-plugin-ui-worker-channel-design.md` 不同的实现决定，写进 T18 的记录第 5 节并说明理由。
- **手工验收**：T18 完成后由用户按记录第 4 节在真实 npm/Homebrew 环境逐条复核；失败项补记并修复后重跑对应门禁。
