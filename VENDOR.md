# qaqh-winui-reactor-vendor

**Vendor snapshot（源码裁剪快照）— 冻结，不跟踪上游。**

本仓库是 `windows-reactor` 下游 fork 的**工作树快照**：已删除全部 git 历史，只保留构建所需源码。

## 来源声明

| 项 | 值 |
|---|---|
| 上游 | `microsoft/windows-rs` |
| 下游 fork | `QAQTam/qaq-winui-reactor`（分支 `deepx-winui`） |
| 冻结 commit | `74a6a4e5d`（最后一次 sync：Windows App SDK 2.4.0 #4835） |
| 冻结日期 | 2026-08-16 |
| 裁剪方式 | `git archive 74a6a4e5d`（无 .git 历史） |
| 保留的下游补丁 | ScrollViewer.scroll_to_bottom 滚动命令组（QAQ-Harness 思考链路追底依赖） |

## 冻结策略

- 消费方（qaqh-winui-app）通过 **git rev 指向本仓库的 commit**，实现秒级拉取。
- 上游更新时：在 `QAQTam/qaq-winui-reactor` 的 `deepx-winui` 分支完成 merge 后，重新执行 `git archive` 生成新的 vendor commit。
- 本仓库不接收功能修改；修改一律回到 fork 分支进行。

## 结构

完整保留 fork 工作树（`crates/libs/*`、`crates/tools/*`、根 workspace Cargo.toml），其中被 QAQ-Harness 消费的包：

- `windows-reactor` / `windows-numerics` / `windows`
- `windows-core` / `windows-collections` / `windows-future` / `windows-time`
- `windows-bindgen` / `windows-reactor-setup`

## License

沿用上游 windows-rs 的 MIT/Apache-2.0 许可。
