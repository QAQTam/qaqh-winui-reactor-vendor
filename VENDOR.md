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

## 冻结策略（2026-08-29 修订：自有组件化）

**上游 `microsoft/windows-rs` 已重写 reactor**，原「fork 分支 merge → 重新 archive」同步链在 fork 层即断裂，
重新同步等同整体移植。经评估**终止同步、本仓库转为自有组件直接维护**（冻结 tag：`frozen/2026-08-29`）：

- 消费方（qaqh-winui-app）通过 path 依赖指向本仓库工作树。
- 功能修改直接落在本仓库（ practice 自 2026-08 下旬已然如此，本条为追认），每次修改更新下方补丁清单。
- 重新评估迁移的唯一条件：重写后的上游给出我们**必须**的能力（当前没有）；届时按「逐补丁移植」立项，不做整体重写。
- ~~本仓库不接收功能修改~~（原策略，已废）。

## 补丁清单（vs 上游快照 `1ee42c9`，403 行 / 11 文件）

| 主题 | 文件 | 说明 |
|---|---|---|
| F-N11 布局隐式动画 | `composition/src/{animation,bindings,bindings_lifted,compositor}.rs`、`reactor/src/backend/winui/mod.rs` | Vector2 关键帧全链 + apply_layout_animation；F-N15 §5 关键锚点 |
| F-N15 STALE 防御 | `reactor/src/reconciler/mod.rs` | pass 后仍脏节点消费脏标记（A 方案），替代 debug_assert 中止 |
| NumberBox 压缩阈值旋钮 | `reactor/src/widgets/number_box.rs`、`backend/winui/{generated_set_prop.rs,bindings.rs}` | SmallChange/LargeChange/SpinButtonPlacementMode（**regen 时需保留**） |
| ScrollViewer 追底 | 快照自带 | scroll_to_bottom 滚动命令组（思考链路追底依赖） |

待办补丁（冻结基线上实施）：Slider 刻度三件套（tick_frequency/tick_placement/snaps_to，FFI 槽位已投影）·
AnimatedIcon+AnimatedVisuals 通道（~150-250 行）· F-R3 删除线 / F-R4 Hyperlink · F-T1 TabView selection key。

## 同步记录（winui-app 侧重写触发）

- 2026-08-21 `sidebar` 简化为纯工作区导航（`TabView` 单一切换）+ `home/startup` 工作区选择器，`vendor` 无代码变更，仅刷新快照 rev 供 `qaqh-winui-app` 重新锁定（`1ee42c9 -> next`）。

## 结构

完整保留 fork 工作树（`crates/libs/*`、`crates/tools/*`、根 workspace Cargo.toml），其中被 QAQ-Harness 消费的包：

- `windows-reactor` / `windows-numerics` / `windows`
- `windows-core` / `windows-collections` / `windows-future` / `windows-time`
- `windows-bindgen` / `windows-reactor-setup`

## License

沿用上游 windows-rs 的 MIT/Apache-2.0 许可。
