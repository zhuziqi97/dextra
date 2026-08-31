# P0-C Cerebro Bridge 纵向协议 Spike

状态：已完成（2026-08-31）

## 结论

固定 Codeg `v0.29.0` / `769610c626f1fc4b18c11d3e289326acf097b99f`
后，可以在不修改 ACP Manager、SessionState、数据库模型和前端 reducer 的前提下，
通过窄 `Transport` 与 Core port 完成：

```text
connect -> cold snapshot -> prompt -> permission event
        -> permission decision -> cancel -> reconnect replay
```

因此 P0-C 的采用结论是继续使用 Codeg 产品协议和 UI，不另造 ACP 远程协议。
平台写操作必须继续由 Cerebro operation/outbox 拥有，Codeg DTO 只留在 vendor
boundary 内。

## 固定边界

- `PROTOCOL_VERSION=1`
- `CODEG_API_REVISION=1`
- Runner `HELLO` 固定报告 `RUNNER_BUILD_ID`、Codeg 上游版本和 commit。
- `cerebro-command-registry.json` 是 Web bundle 与 Rust Bridge 共用的唯一
  command/channel 分类真源；两侧只各自解析同一文件，不维护第二份全集。
- `acp_connect/acp_prompt/acp_cancel/acp_disconnect` 和三种用户响应先进入
  `SESSION_CREATE/TASK_START/TASK_CANCEL/SESSION_CLOSE/APPROVAL_DECIDE`。
- 只有登记的 Session/Runtime read 与两个全局只读 channel 可 Relay；未知命令和
  `FORGE_MUTATE/LOCAL_OS_INTEGRATION/TERMINAL/SETTINGS/CREDENTIALS/INSTALL`
  默认拒绝。
- Web peer 接口没有 Runner URL、Codeg bearer、设备 token 或 task ticket 字段；
  secret/config/install/update 命令在浏览器 Transport 边界即失败，Runner Bridge
  还会独立执行同一 registry 分类，不能靠调用通用 RPC 绕过 operation。
- `codeg-server` 默认绑定 `127.0.0.1`，仍允许 owner 显式设置 `CODEG_HOST`。
- 桌面和 server 更新源统一指向 `.invalid` 保留域名，P0-C 不会获取或安装 Codeg
  上游发行包；P1 建立 Dextra 自有签名发行源后再替换。

## 实测协议与状态转换

| 步骤 | 外层消息/Transport 调用 | 可观察结果 |
|---|---|---|
| 握手 | `HELLO` | revision、上游版本或 commit 不一致时返回稳定错误并 fail fast |
| 连接 | `acp_connect -> SESSION_CREATE -> SESSION_OPEN` | fake Core 建立 `codeg-connection-1` |
| 冷 attach | `CODEG_STREAM_ATTACH`，无 cursor | 返回 snapshot 与 `event_seq=0` |
| prompt | `acp_prompt -> TASK_START` | fake ACP 发出 `permission_request(seq=1)` |
| 审批 | `acp_respond_permission -> APPROVAL_DECIDE` | 发出 `permission_resolved(seq=2)` |
| 取消 | `acp_cancel -> TASK_CANCEL` | 发出取消后的连接状态事件；重复 `COMMAND_ID` 只返回 duplicate ACK，不重复副作用 |
| 断线重连 | 以最后应用的 `seq=3` 自动 reattach | replay 断线窗口内的 `seq=4`，不重复投递旧事件 |

已观察的稳定 Bridge/Transport 错误包括：

- `PROTOCOL_VERSION_UNSUPPORTED`
- `CODEG_API_REVISION_UNSUPPORTED`
- `CODEG_UPSTREAM_MISMATCH`
- `INVALID_ENVELOPE`
- `CODEG_COMMAND_NOT_REMOTE`
- `CODEG_COMMAND_REMOTE_DENIED`
- `CODEG_COMMAND_REQUIRES_OPERATION`
- `CODEG_CHANNEL_NOT_REMOTE`
- `CORE_OPERATION_FAILED`

P0-C dispatcher 只用内存记录演示 `COMMAND_ID` 去重和 ACK，且没有接入 daemon
bootstrap，因此不是生产状态 owner。P1 必须把“已持久接收”的 ACK 语义接到 Runner
本地 command inbox/唯一键；不能把这个内存表包装成生产持久层。

## P0-B 需要冻结的实测发现

1. 当前计划的平台到 Runner 消息表缺少 session 创建消息，但 `acp_connect` 是写操作，
   不能进入通用 `CODEG_RPC_REQUEST`。候选协议新增 `SESSION_OPEN`，由平台
   `SESSION_CREATE` operation/outbox 产生；P0-B 应确认名称和必填字段。
2. `LiveSessionSnapshot/EventEnvelope` 可原样支撑 UI 的 snapshot、permission 和
   replay，不应复制进 Cerebro 公共 REST/领域 DTO。
3. 全局 channel 没有 stream cursor 语义，重连后的恢复仍应重新 query list/snapshot，
   不能假设 channel replay。
4. ACK 与执行结果必须分开：生产 ACK 表示 Runner 已持久接收，Task 状态/错误表示
   执行进度。本 Spike 的同步 `DispatchResult` 只是合同最小证据。
5. 平台 adapter 目前只存在于测试 fake。5.9.0 合入 `main` 后，P1 才从 Convene
   最新 `main` 建立真实 operation/outbox/Relay 切片。

## 补丁分布

- Bridge 和协议集中在 `src-tauri/src/cerebro_bridge/`。
- Web vendor boundary 集中在 `src/lib/transport/cerebro-*`，只在现有 transport
  index 增加选择入口。
- 发行边界只修改 `codeg_server.rs` 默认 host、`version.rs` 和
  `tauri.conf.json` 更新地址。
- fake platform、fake Relay 和 fake ACP 全在合同测试中，没有真实账号或平台依赖。
- ACP Manager、SessionState、数据库、现有 event reducer 的差异均为 0。

## 验证记录

```text
pnpm exec vitest run src/lib/transport/cerebro-remote-transport.test.ts
  1 file / 3 tests passed

pnpm test -- src/lib/transport/cerebro-remote-transport.test.ts
  实际执行全量：375 files / 5292 tests passed

pnpm exec tsc --noEmit
  passed

cargo test --manifest-path src-tauri/Cargo.toml --no-default-features cerebro_bridge --lib
  6 passed

cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  p0_c_update_source_is_explicitly_unavailable --lib
  1 passed

cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  --bin codeg-server dextra_server_defaults_to_loopback
  1 passed
```

默认 feature 的首次 Rust 测试在编译 Bridge 前被上游 Tauri build script 拒绝，真实原因是
仓库尚未生成 `../out`。随后使用仓库已有 headless server 路径
`--no-default-features` 完成同一库和 bin 验证，没有为测试伪造前端产物。

## 上游与 rebase 记录

2026-08-31 执行 `git fetch upstream --tags --prune` 后：

- `upstream/main = v0.29.0 = 769610c626f1fc4b18c11d3e289326acf097b99f`
- `merge-base(main, upstream/main)` 为同一 commit。
- 当前没有 `v0.29.0` 之后的稳定 tag，无法声称已经验证“下一稳定 tag”的真实冲突。
- 在 Spike 提交 `e9c8da22` 上执行首次 `git rebase upstream/main`，Git 返回
  `Current branch main is up to date.`，冲突数为 0；这是当前同基线演练，不冒充
  下一稳定 tag 的升级证据。
- 下一稳定 tag 出现时必须再次演练，记录真实冲突文件和 Bridge 外差异，才能作为
  升级采用证据。
